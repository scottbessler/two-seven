use crate::{
    app::AppState,
    bank::AccountOwner,
    error::AppError,
    holdem::Action,
    render,
    session::{AuthUser, MaybeUser},
    table::{BotKind, SeatOccupant, Stakes, Table, TableMode},
    view::table_view,
};
use axum::{
    Json,
    extract::{Path, State},
    response::{
        Html, IntoResponse,
        sse::{Event, Sse},
    },
};
use chrono::Utc;
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
    Html(render::home(current))
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
    Ok(Html(render::table_page(&table_view(
        &table,
        Some(viewer.unwrap_or(usize::MAX)),
    ))))
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
    Ok(Json(table_view(&table, viewer)))
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
        serde_json::to_string(&table_view(&table, viewer)).map_err(AppError::internal)?
    };
    let rx = s.tables.subscribe();
    let tables = s.tables.clone();
    let events = stream::unfold((Some(snapshot), rx), move |(first, mut rx)| {
        let tables = tables.clone();
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
                            let data=serde_json::to_string(&table_view(&table,user.and_then(|uid|table.seats.iter().position(|seat|matches!(seat.occupant,SeatOccupant::Human{user_id} if user_id==uid))))).unwrap_or_else(|_|"{}".into());
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

fn maybe_start_hand(table: &mut Table) {
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
    let stacks: Vec<(usize, i64)> = table
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
    table.hand = Some(crate::holdem::Hand::new_with_seats(
        table.stakes,
        &stacks,
        table.button,
        table.hand_no,
    ));
    table.next_action_at = None;
}
fn settle_finished_hand(table: &mut Table) {
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
    table.button = (table.button + 1) % table.seats.len();
    table.last_hand = hand.summary;
    table.next_action_at = Some(Utc::now() + chrono::Duration::seconds(3));
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
    let (no_debt, min, max) = {
        let t = table_arc.lock().await;
        let no_debt = matches!(t.mode, TableMode::Cash { no_debt: true });
        (no_debt, t.min_buy_in, t.max_buy_in)
    };
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
    let (seat, stack) = {
        let t = table.lock().await;
        t.seats
            .iter()
            .enumerate()
            .find_map(|(i, seat)| {
                matches!(seat.occupant,SeatOccupant::Human{user_id} if user_id==user)
                    .then_some((i, seat.stack))
            })
            .ok_or_else(|| AppError::bad_request("you are not seated"))?
    };
    s.bank
        .cash_out(AccountOwner::User(user), id, stack)
        .await
        .map_err(AppError::internal)?;
    s.tables
        .update(id, |t| {
            t.seats[seat].occupant = SeatOccupant::Empty;
            t.seats[seat].stack = 0;
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
    let (no_debt, max) = {
        let table = table.lock().await;
        (
            matches!(table.mode, TableMode::Cash { no_debt: true }),
            table.max_buy_in,
        )
    };
    if input.amount <= 0 || input.amount > max {
        return Err(AppError::bad_request("invalid rebuy amount"));
    }
    s.bank
        .buy_in(AccountOwner::User(user), id, input.amount, no_debt)
        .await
        .map_err(|_| AppError::bad_request("insufficient funds"))?;
    let result = s.tables.update(id, |table| { let seat = table.seats.iter_mut().find(|seat| matches!(seat.occupant, SeatOccupant::Human { user_id } if user_id == user)).ok_or_else(|| anyhow::anyhow!("you are not seated"))?; seat.stack += input.amount; Ok(()) }).await;
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
    pub kind: Option<BotKind>,
}
pub async fn bot_table(
    AuthUser(_user): AuthUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<BotRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    s.tables
        .update(id, |table| {
            let seat = table
                .seats
                .get_mut(input.seat)
                .ok_or_else(|| anyhow::anyhow!("invalid seat"))?;
            seat.occupant = input
                .kind
                .map_or(SeatOccupant::Empty, |kind| SeatOccupant::Bot { kind });
            if matches!(seat.occupant, SeatOccupant::Empty) {
                seat.stack = 0;
            }
            maybe_start_hand(table);
            Ok(())
        })
        .await
        .map_err(AppError::internal)?;
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
