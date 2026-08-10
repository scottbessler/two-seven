use crate::{
    app::AppState,
    bank::AccountOwner,
    error::AppError,
    holdem::Action,
    render,
    session::{AuthUser, MaybeUser},
    table::{
        BlindLevel, BotKind, SeatOccupant, Stakes, Table, TableMode, TournamentConfig,
        TournamentState, maybe_start_hand, settle_finished_hand,
    },
    view::{LobbyTableView, LobbyTournamentView, table_view_with_banks},
};
use axum::{
    Json,
    extract::{Path, State},
    response::{
        Html, IntoResponse,
        sse::{Event, Sse},
    },
};
use futures_util::stream;
use serde::Deserialize;
use std::{convert::Infallible, time::Duration};
use uuid::Uuid;

pub async fn healthcheck() -> &'static str {
    "OK"
}
pub async fn index(State(s): State<AppState>, MaybeUser(user): MaybeUser) -> Html<String> {
    let current = match user {
        Some(id) => s.users.get(id).await.map(|u| (id, u.display_name)),
        None => None,
    };
    if let Some((id, name)) = current {
        Html(render::home_lobby(&name, &lobby_views(&s, id).await))
    } else {
        Html(render::home(None))
    }
}

#[derive(Deserialize)]
pub struct CreateTable {
    pub name: String,
    pub stakes: Stakes,
    pub no_debt: Option<bool>,
    pub max_seats: Option<usize>,
    pub min_buy_in: Option<i64>,
    pub max_buy_in: Option<i64>,
}
pub async fn new_table() -> Html<String> {
    Html(render::table_create())
}
pub async fn new_tournament() -> Html<String> {
    Html(render::tournament_create())
}

#[derive(Deserialize)]
pub struct CreateTournament {
    pub name: String,
    pub buy_in: i64,
    pub seat_count: usize,
    pub starting_chips: i64,
    pub no_debt: Option<bool>,
    pub levels: Vec<BlindLevel>,
    pub payout_percentages: Vec<u8>,
}

pub async fn create_tournament(
    AuthUser(_user): AuthUser,
    State(s): State<AppState>,
    Json(input): Json<CreateTournament>,
) -> Result<Json<serde_json::Value>, AppError> {
    if input.buy_in <= 0
        || input.starting_chips <= 0
        || input.seat_count < 2
        || input.levels.is_empty()
        || input.payout_percentages.is_empty()
        || input
            .payout_percentages
            .iter()
            .map(|value| u16::from(*value))
            .sum::<u16>()
            != 100
    {
        return Err(AppError::bad_request(
            "invalid tournament configuration or payouts",
        ));
    }
    let level = input.levels[0].clone();
    let config = TournamentConfig {
        buy_in: input.buy_in,
        seat_count: input.seat_count.clamp(2, 9),
        starting_chips: input.starting_chips,
        levels: input.levels,
        payout_percentages: input.payout_percentages,
        no_debt: input.no_debt.unwrap_or(false),
    };
    let table = Table::new(
        input.name,
        Stakes::NoLimit {
            small_blind: level.small_blind,
            big_blind: level.big_blind,
        },
        TableMode::Tournament(TournamentState {
            config: config.clone(),
            current_level: 0,
            hands_at_level: 0,
            finish_order: Vec::new(),
            registered: 0,
            started: false,
            prize_pool: 0,
            finished: false,
            paid_out: false,
        }),
        config.seat_count,
        config.starting_chips,
        config.starting_chips,
    );
    let id = s.tables.insert(table).await.map_err(AppError::internal)?;
    Ok(Json(
        serde_json::json!({"id":id,"url":format!("/tables/{id}")}),
    ))
}

pub async fn register_tournament(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<JoinRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let table = s
        .tables
        .get(id)
        .await
        .ok_or_else(|| AppError::not_found("tournament not found"))?;
    let (config, occupied, started) = {
        let table = table.lock().await;
        let TableMode::Tournament(state) = &table.mode else {
            return Err(AppError::bad_request("not a tournament"));
        };
        (
            state.config.clone(),
            table
                .seats
                .iter()
                .filter(|seat| !matches!(seat.occupant, SeatOccupant::Empty))
                .count(),
            state.started || state.finished,
        )
    };
    if input.buy_in != config.buy_in || started || occupied >= config.seat_count || {
        let table = table.lock().await;
        table
            .seats
            .iter()
            .any(|seat| matches!(seat.occupant, SeatOccupant::Human { user_id } if user_id == user))
    } {
        return Err(AppError::bad_request("registration is unavailable"));
    }
    s.bank
        .buy_in(AccountOwner::User(user), id, config.buy_in, config.no_debt)
        .await
        .map_err(|_| AppError::bad_request("insufficient funds"))?;
    let result = s
        .tables
        .update(id, |table| {
            let TableMode::Tournament(state) = &mut table.mode else {
                return Err(anyhow::anyhow!("not a tournament"));
            };
            if input.seat >= table.seats.len()
                || !matches!(table.seats[input.seat].occupant, SeatOccupant::Empty)
            {
                return Err(anyhow::anyhow!("seat is unavailable"));
            }
            table.seats[input.seat] = crate::table::Seat {
                occupant: SeatOccupant::Human { user_id: user },
                stack: state.config.starting_chips,
                sitting_out: false,
                pending_departure: false,
            };
            state.registered += 1;
            state.prize_pool += state.config.buy_in;
            if state.registered == state.config.seat_count {
                maybe_start_hand(table);
            }
            Ok(())
        })
        .await;
    if let Err(error) = result {
        let _ = s
            .bank
            .cash_out(AccountOwner::User(user), id, config.buy_in)
            .await;
        return Err(AppError::internal(error));
    }
    Ok(Json(serde_json::json!({"ok":true})))
}
pub async fn tables(AuthUser(user): AuthUser, State(s): State<AppState>) -> Html<String> {
    Html(render::lobby(&lobby_views(&s, user).await))
}

async fn lobby_views(state: &AppState, user: Uuid) -> Vec<LobbyTableView> {
    let mut tables = Vec::new();
    for id in state.tables.ids().await {
        if let Some(table) = state.tables.get(id).await {
            let table = table.lock().await;
            let tournament = match &table.mode {
                TableMode::Tournament(state) => Some(LobbyTournamentView {
                    buy_in: state.config.buy_in,
                    registered: state.registered,
                    seat_count: state.config.seat_count,
                    finished: state.finished,
                    paid_out: state.paid_out,
                }),
                TableMode::Cash { .. } => None,
            };
            let no_debt = match &table.mode {
                TableMode::Cash { no_debt } => *no_debt,
                TableMode::Tournament(state) => state.config.no_debt,
            };
            tables.push(LobbyTableView {
                id,
                name: table.name.clone(),
                stakes: table.stakes,
                occupied: table
                    .seats
                    .iter()
                    .filter(|seat| !matches!(seat.occupant, SeatOccupant::Empty))
                    .count(),
                max_seats: table.max_seats,
                no_debt,
                tournament,
                your_seat: table.seats.iter().position(|seat| {
                    matches!(seat.occupant, SeatOccupant::Human { user_id } if user_id == user)
                }),
            });
        }
    }
    tables
}
pub async fn create_table(
    AuthUser(_user): AuthUser,
    State(s): State<AppState>,
    Json(input): Json<CreateTable>,
) -> Result<impl IntoResponse, AppError> {
    let mode = TableMode::Cash {
        no_debt: input.no_debt.unwrap_or(false),
    };
    let table = Table::new(
        input.name,
        input.stakes,
        mode,
        input.max_seats.unwrap_or(9).clamp(2, 9),
        input.min_buy_in.unwrap_or(100),
        input.max_buy_in.unwrap_or(10_000),
    );
    let id = s.tables.insert(table).await.map_err(AppError::internal)?;
    Ok(Json(
        serde_json::json!({"id":id,"url":format!("/tables/{id}")}),
    ))
}

pub async fn table_page(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Html<String>, AppError> {
    let table = s
        .tables
        .get(id)
        .await
        .ok_or_else(|| AppError::not_found("table not found"))?;
    let table = table.lock().await;
    let viewer = table.seats.iter().position(
        |seat| matches!(seat.occupant, SeatOccupant::Human { user_id } if user_id == user),
    );
    let banks = seat_banks(&s, &table).await;
    let names = seat_names(&s, &table).await;
    Ok(Html(render::table_page(&table_view_with_banks(
        &table, viewer, &banks, &names,
    ))))
}

async fn seat_names(state: &AppState, table: &Table) -> std::collections::HashMap<usize, String> {
    let mut names = std::collections::HashMap::new();
    for (index, seat) in table.seats.iter().enumerate() {
        if let SeatOccupant::Human { user_id } = seat.occupant
            && let Some(user) = state.users.get(user_id).await
        {
            names.insert(index, user.display_name);
        }
    }
    names
}

async fn seat_banks(
    state: &AppState,
    table: &Table,
) -> std::collections::HashMap<usize, crate::bank::Account> {
    let mut banks = std::collections::HashMap::new();
    for (index, seat) in table.seats.iter().enumerate() {
        let owner = match seat.occupant {
            SeatOccupant::Human { user_id } => Some(AccountOwner::User(user_id)),
            SeatOccupant::Bot { kind } => Some(AccountOwner::Bot(kind)),
            SeatOccupant::Empty => None,
        };
        if let Some(owner) = owner
            && let Ok(account) = state.bank.account(owner).await
        {
            banks.insert(index, account);
        }
    }
    banks
}
pub async fn table_state(
    MaybeUser(user): MaybeUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<crate::view::TableView>, AppError> {
    let table = s
        .tables
        .get(id)
        .await
        .ok_or_else(|| AppError::not_found("table not found"))?;
    let table = table.lock().await;
    let viewer = user.and_then(|uid| {
        table.seats.iter().position(
            |seat| matches!(seat.occupant, SeatOccupant::Human { user_id } if user_id == uid),
        )
    });
    let banks = seat_banks(&s, &table).await;
    let names = seat_names(&s, &table).await;
    Ok(Json(table_view_with_banks(&table, viewer, &banks, &names)))
}

pub async fn table_events(
    MaybeUser(user): MaybeUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, AppError> {
    let table = s
        .tables
        .get(id)
        .await
        .ok_or_else(|| AppError::not_found("table not found"))?;
    let snapshot = {
        let table = table.lock().await;
        let viewer = user.and_then(|uid| {
            table.seats.iter().position(
                |seat| matches!(seat.occupant, SeatOccupant::Human { user_id } if user_id == uid),
            )
        });
        let banks = seat_banks(&s, &table).await;
        let names = seat_names(&s, &table).await;
        serde_json::to_string(&table_view_with_banks(&table, viewer, &banks, &names))
            .map_err(AppError::internal)?
    };
    let rx = s.tables.subscribe();
    let tables = s.tables.clone();
    let state = s.clone();
    let events = stream::unfold((Some(snapshot), rx), move |(first, mut rx)| {
        let tables = tables.clone();
        let state = state.clone();
        async move {
            if let Some(snapshot) = first {
                return Some((
                    Ok(Event::default().event("state").data(snapshot)),
                    (None, rx),
                ));
            }
            loop {
                match rx.recv().await {
                    Ok(changed) if changed == id => {
                        if let Some(table) = tables.get(id).await {
                            let table = table.lock().await;
                            let viewer = user.and_then(|uid|table.seats.iter().position(|seat|matches!(seat.occupant,SeatOccupant::Human{user_id} if user_id==uid)));
                            let banks = seat_banks(&state, &table).await;
                            let names = seat_names(&state, &table).await;
                            let data = serde_json::to_string(&table_view_with_banks(
                                &table, viewer, &banks, &names,
                            ))
                            .unwrap_or_else(|_| "{}".into());
                            return Some((
                                Ok(Event::default().event("state").data(data)),
                                (None, rx),
                            ));
                        }
                    }
                    Ok(_) => continue,
                    Err(_) => return None,
                }
            }
        }
    });
    Ok(Sse::new(events)
        .keep_alive(axum::response::sse::KeepAlive::new().interval(Duration::from_secs(15))))
}

#[derive(Deserialize)]
pub struct JoinRequest {
    pub seat: usize,
    pub buy_in: i64,
}
pub async fn join_table(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<JoinRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let table_arc = s
        .tables
        .get(id)
        .await
        .ok_or_else(|| AppError::not_found("table not found"))?;
    let (no_debt, min, max, tournament) = {
        let t = table_arc.lock().await;
        let no_debt = matches!(t.mode, TableMode::Cash { no_debt: true });
        (
            no_debt,
            t.min_buy_in,
            t.max_buy_in,
            matches!(t.mode, TableMode::Tournament(_)),
        )
    };
    if tournament {
        return Err(AppError::bad_request(
            "tournament registration uses /tournaments/{id}/register",
        ));
    }
    if input.buy_in < min || input.buy_in > max {
        return Err(AppError::bad_request("buy-in is outside the table limits"));
    }
    s.bank
        .buy_in(AccountOwner::User(user), id, input.buy_in, no_debt)
        .await
        .map_err(|_| AppError::bad_request("insufficient funds"))?;
    let result = s
        .tables
        .update(id, |t| {
            if input.seat >= t.seats.len() {
                return Err(anyhow::anyhow!("invalid seat"));
            }
            if !matches!(t.seats[input.seat].occupant, SeatOccupant::Empty) {
                return Err(anyhow::anyhow!("seat is occupied"));
            }
            t.seats[input.seat] = crate::table::Seat {
                occupant: SeatOccupant::Human { user_id: user },
                stack: input.buy_in,
                sitting_out: false,
                pending_departure: false,
            };
            maybe_start_hand(t);
            Ok(())
        })
        .await;
    if let Err(error) = result {
        let _ = s
            .bank
            .cash_out(AccountOwner::User(user), id, input.buy_in)
            .await;
        return Err(AppError::internal(error));
    }
    Ok(Json(serde_json::json!({"ok":true})))
}

pub async fn leave_table(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let table = s
        .tables
        .get(id)
        .await
        .ok_or_else(|| AppError::not_found("table not found"))?;
    let (seat, stack, tournament, live_hand) = {
        let mut t = table.lock().await;
        if t.hand.as_ref().is_some_and(|hand| hand.complete) {
            settle_finished_hand(&mut t);
        }
        let (seat, stack) = t
            .seats
            .iter()
            .enumerate()
            .find_map(|(i, seat)| {
                matches!(seat.occupant,SeatOccupant::Human{user_id} if user_id==user)
                    .then_some((i, seat.stack))
            })
            .ok_or_else(|| AppError::bad_request("you are not seated"))?;
        (
            seat,
            stack,
            matches!(t.mode, TableMode::Tournament(_)),
            t.hand.is_some(),
        )
    };
    if live_hand {
        s.tables
            .update(id, |t| {
                if let Some(hand) = t.hand.as_mut() {
                    if hand.current_player == Some(seat) {
                        hand.apply_action(Action::Fold)
                            .map_err(|e| anyhow::anyhow!(e))?;
                    } else if let Some(player) = hand.players.iter_mut().find(|p| p.seat == seat) {
                        player.folded = true;
                    }
                }
                let seat = t
                    .seats
                    .get_mut(seat)
                    .ok_or_else(|| anyhow::anyhow!("seat missing"))?;
                seat.sitting_out = true;
                seat.pending_departure = true;
                Ok(())
            })
            .await
            .map_err(AppError::internal)?;
        return Ok(Json(serde_json::json!({"ok":true,"pending":true})));
    }
    if !tournament {
        s.bank
            .cash_out(AccountOwner::User(user), id, stack)
            .await
            .map_err(AppError::internal)?;
    }
    s.tables
        .update(id, |t| {
            t.seats[seat].occupant = SeatOccupant::Empty;
            t.seats[seat].stack = 0;
            t.seats[seat].pending_departure = false;
            Ok(())
        })
        .await
        .map_err(AppError::internal)?;
    Ok(Json(serde_json::json!({"ok":true})))
}

#[derive(Deserialize)]
pub struct SitRequest {
    pub sitting_out: bool,
}
pub async fn sit_table(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<SitRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    s.tables
        .update(id, |t| {
            let seat = t
                .seats
                .iter_mut()
                .find(|seat| matches!(seat.occupant,SeatOccupant::Human{user_id} if user_id==user))
                .ok_or_else(|| anyhow::anyhow!("you are not seated"))?;
            seat.sitting_out = input.sitting_out;
            Ok(())
        })
        .await
        .map_err(AppError::internal)?;
    Ok(Json(serde_json::json!({"ok":true})))
}

#[derive(Deserialize)]
pub struct ActionRequest {
    pub kind: String,
    pub amount: Option<i64>,
}
pub async fn action(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<ActionRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    s.tables
        .update(id, |t| {
            let seat = t
                .seats
                .iter()
                .position(
                    |seat| matches!(seat.occupant,SeatOccupant::Human{user_id} if user_id==user),
                )
                .ok_or_else(|| anyhow::anyhow!("you are not seated"))?;
            let hand = t
                .hand
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("no hand in progress"))?;
            let action = match input.kind.as_str() {
                "fold" => Action::Fold,
                "check" => Action::Check,
                "call" => Action::Call,
                "all_in" => Action::AllIn,
                "bet" => Action::Bet {
                    amount: input
                        .amount
                        .ok_or_else(|| anyhow::anyhow!("amount required"))?,
                },
                "raise" => Action::Raise {
                    amount: input
                        .amount
                        .ok_or_else(|| anyhow::anyhow!("amount required"))?,
                },
                _ => return Err(anyhow::anyhow!("unknown action")),
            };
            if hand.current_player != Some(seat) {
                return Err(anyhow::anyhow!("not your turn"));
            }
            hand.apply_action(action).map_err(|e| anyhow::anyhow!(e))?;
            let complete = hand.complete;
            if complete {
                settle_finished_hand(t);
            }
            Ok(())
        })
        .await
        .map_err(AppError::internal)?;
    Ok(Json(serde_json::json!({"ok":true})))
}

#[allow(dead_code)]
#[derive(Deserialize)]
pub struct RebuyRequest {
    pub amount: i64,
}
pub async fn rebuy_table(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<RebuyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let table = s
        .tables
        .get(id)
        .await
        .ok_or_else(|| AppError::not_found("table not found"))?;
    let (no_debt, max, tournament) = {
        let table = table.lock().await;
        (
            matches!(table.mode, TableMode::Cash { no_debt: true }),
            table.max_buy_in,
            matches!(table.mode, TableMode::Tournament(_)),
        )
    };
    if tournament {
        return Err(AppError::bad_request("tournament chips cannot be rebought"));
    }
    {
        let table = table.lock().await;
        if table.hand.is_some() {
            return Err(AppError::bad_request(
                "rebuy is unavailable while a hand is in progress",
            ));
        }
    }
    if input.amount <= 0 || input.amount > max {
        return Err(AppError::bad_request("invalid rebuy amount"));
    }
    s.bank
        .buy_in(AccountOwner::User(user), id, input.amount, no_debt)
        .await
        .map_err(|_| AppError::bad_request("insufficient funds"))?;
    let result = s
        .tables
        .update(id, |table| {
            if table.hand.is_some() {
                return Err(anyhow::anyhow!(
                    "rebuy is unavailable while a hand is in progress"
                ));
            }
            let seat = table
                .seats
                .iter_mut()
                .find(|seat| {
                    matches!(seat.occupant, SeatOccupant::Human { user_id } if user_id == user)
                })
                .ok_or_else(|| anyhow::anyhow!("you are not seated"))?;
            seat.stack += input.amount;
            Ok(())
        })
        .await;
    if let Err(error) = result {
        let _ = s
            .bank
            .cash_out(AccountOwner::User(user), id, input.amount)
            .await;
        return Err(AppError::internal(error));
    }
    Ok(Json(serde_json::json!({"ok":true})))
}

#[derive(Deserialize)]
pub struct BotRequest {
    pub seat: usize,
    pub kind: Option<String>,
    pub buy_in: Option<i64>,
}
pub async fn bot_table(
    AuthUser(_user): AuthUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<BotRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let table = s
        .tables
        .get(id)
        .await
        .ok_or_else(|| AppError::not_found("table not found"))?;
    let (old, min, max, started) = {
        let table = table.lock().await;
        let seat = table
            .seats
            .get(input.seat)
            .ok_or_else(|| AppError::bad_request("invalid seat"))?;
        (
            seat.occupant.clone(),
            table.min_buy_in,
            table.max_buy_in,
            matches!(&table.mode, TableMode::Tournament(state) if state.started || state.finished),
        )
    };
    let tournament = {
        let table = table.lock().await;
        matches!(table.mode, TableMode::Tournament(_))
    };
    if tournament && started {
        return Err(AppError::bad_request(
            "tournament seats cannot be changed after registration closes",
        ));
    }
    if tournament && input.kind.is_some() && !matches!(old, SeatOccupant::Empty) {
        return Err(AppError::bad_request("tournament seats cannot be replaced"));
    }
    if let SeatOccupant::Bot { kind } = old {
        let stack = s.tables.get(id).await.unwrap().lock().await.seats[input.seat].stack;
        if !tournament {
            s.bank
                .cash_out(AccountOwner::Bot(kind), id, stack)
                .await
                .map_err(AppError::internal)?;
        }
    }
    let mut bought = None;
    if let Some(kind_name) = input.kind {
        let kind = kind_name
            .parse::<BotKind>()
            .map_err(AppError::bad_request)?;
        let amount = if tournament {
            let table = s
                .tables
                .get(id)
                .await
                .ok_or_else(|| AppError::not_found("table not found"))?;
            let table = table.lock().await;
            match &table.mode {
                TableMode::Tournament(state) => state.config.buy_in,
                TableMode::Cash { .. } => input.buy_in.unwrap_or(min),
            }
        } else {
            input.buy_in.unwrap_or(min)
        };
        if !tournament && (amount < min || amount > max) {
            return Err(AppError::bad_request("bot buy-in is outside table limits"));
        }
        s.bank
            .buy_in(
                AccountOwner::Bot(kind),
                id,
                amount,
                if tournament {
                    let table = s.tables.get(id).await.unwrap();
                    let table = table.lock().await;
                    match &table.mode {
                        TableMode::Tournament(state) => state.config.no_debt,
                        TableMode::Cash { .. } => false,
                    }
                } else {
                    false
                },
            )
            .await
            .map_err(|_| AppError::bad_request("insufficient funds"))?;
        bought = Some((kind, amount));
    }
    let result = s
        .tables
        .update(id, |table| {
            let seat = table
                .seats
                .get_mut(input.seat)
                .ok_or_else(|| anyhow::anyhow!("invalid seat"))?;
            if let Some((kind, amount)) = bought {
                seat.occupant = SeatOccupant::Bot { kind };
                seat.stack = if tournament {
                    match &table.mode {
                        TableMode::Tournament(state) => state.config.starting_chips,
                        TableMode::Cash { .. } => amount,
                    }
                } else {
                    amount
                };
                seat.sitting_out = false;
                seat.pending_departure = false;
                if let TableMode::Tournament(state) = &mut table.mode {
                    state.registered += 1;
                    state.prize_pool += state.config.buy_in;
                }
            } else {
                seat.occupant = SeatOccupant::Empty;
                seat.stack = 0;
                seat.sitting_out = false;
            }
            maybe_start_hand(table);
            Ok(())
        })
        .await;
    if let Err(error) = result {
        if let Some((kind, amount)) = bought {
            let _ = s.bank.cash_out(AccountOwner::Bot(kind), id, amount).await;
        }
        return Err(AppError::internal(error));
    }
    Ok(Json(serde_json::json!({"ok":true})))
}

pub async fn bank_state(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
) -> Result<Json<crate::bank::Account>, AppError> {
    s.bank
        .account(AccountOwner::User(user))
        .await
        .map(Json)
        .map_err(AppError::internal)
}
