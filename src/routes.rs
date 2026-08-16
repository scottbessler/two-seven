use crate::{
    app::AppState,
    bank::AccountOwner,
    blackjack::BlackjackError,
    blitz::{BlitzAnswerError, BlitzDifficulty},
    error::AppError,
    holdem::Action,
    money::{valid_game_amount, valid_optional_game_amount},
    render,
    session::{AuthUser, MaybeUser},
    table::{
        BlindLevel, BotKind, SeatOccupant, Stakes, Table, TableMode, TournamentConfig,
        TournamentState, maybe_start_hand, settle_finished_hand,
    },
    view::{LobbyTableView, LobbyTournamentView, table_view_with_banks},
};
use axum::{
    Form, Json,
    extract::{Path, State},
    http::header,
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

const SERVICE_WORKER_JS: &str = include_str!("../public/sw.js");

pub async fn healthcheck() -> &'static str {
    "OK"
}
pub async fn service_worker() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        SERVICE_WORKER_JS,
    )
}
pub async fn card_test() -> Html<String> {
    Html(render::card_test())
}
pub async fn blackjack(AuthUser(_user): AuthUser) -> Html<String> {
    Html(render::blackjack())
}

pub async fn admin_page(State(_s): State<AppState>) -> Html<String> {
    Html(render::admin(None, None))
}

#[derive(Deserialize)]
pub struct AdminAction {
    pub password: String,
    pub action: String,
}

pub async fn admin_action(
    State(s): State<AppState>,
    Form(input): Form<AdminAction>,
) -> Result<Html<String>, AppError> {
    if !crate::admin::password_matches(&s.admin_password, input.password.trim()) {
        return Ok(Html(render::admin(
            Some("That password did not unlock admin."),
            None,
        )));
    }
    let message = match input.action.as_str() {
        "money" => {
            let report = crate::admin::reset_money_and_loans(&s)
                .await
                .map_err(AppError::internal)?;
            format!(
                "Reset money and loans: {} accounts cleared, {} tables reset, {} humans kicked out.",
                report.accounts, report.tables, report.humans_kicked
            )
        }
        "poker" => {
            let removed = s.stats.reset_all().await.map_err(AppError::internal)?;
            format!("Reset poker stats for {removed} players.")
        }
        "blitz" => {
            let removed = s.blitz.reset_stats().await.map_err(AppError::internal)?;
            format!("Reset blitz stats for {removed} players.")
        }
        _ => return Err(AppError::bad_request("unknown admin action")),
    };
    Ok(Html(render::admin(None, Some(&message))))
}
pub async fn index(State(s): State<AppState>, MaybeUser(user): MaybeUser) -> Html<String> {
    let current = match user {
        Some(id) => s.users.get(id).await.map(|u| (id, u.display_name)),
        None => None,
    };
    if let Some((id, name)) = current {
        Html(render::home_lobby(
            &name,
            &lobby_views(&s, id).await,
            balance_of(&s, id).await,
        ))
    } else {
        Html(render::home(None))
    }
}

pub async fn new_table(AuthUser(user): AuthUser, State(s): State<AppState>) -> Html<String> {
    Html(render::table_create(balance_of(&s, user).await))
}
pub async fn new_tournament(AuthUser(user): AuthUser, State(s): State<AppState>) -> Html<String> {
    Html(render::tournament_create(balance_of(&s, user).await))
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
    if !valid_game_amount(input.buy_in)
        || !valid_game_amount(input.starting_chips)
        || input.seat_count < 2
        || input.levels.is_empty()
        || input.payout_percentages.is_empty()
        || input.levels.iter().any(|level| {
            !valid_game_amount(level.small_blind)
                || !valid_game_amount(level.big_blind)
                || level.big_blind < level.small_blind
                || !valid_optional_game_amount(level.ante)
                || level.hands == 0
        })
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
        config.buy_in,
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
    Json(_input): Json<EmptyRequest>,
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
    if started || occupied >= config.seat_count || {
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
            if state.started
                || state.finished
                || table.seats.iter().any(
                    |seat| matches!(seat.occupant, SeatOccupant::Human { user_id } if user_id == user),
                )
            {
                return Err(anyhow::anyhow!("registration is unavailable"));
            }
            let seat = table
                .seats
                .iter()
                .position(|seat| matches!(seat.occupant, SeatOccupant::Empty))
                .ok_or_else(|| anyhow::anyhow!("registration is unavailable"))?;
            table.seats[seat] = crate::table::Seat {
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
    Html(render::lobby(
        &lobby_views(&s, user).await,
        balance_of(&s, user).await,
    ))
}

#[derive(Deserialize)]
pub struct BlackjackStartRequest {
    pub bet: i64,
}

pub async fn blackjack_start(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Json(input): Json<BlackjackStartRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // You may bet anything you can cover, up to the whole bankroll.
    let balance = balance_of(&s, user).await;
    if !valid_game_amount(input.bet) || input.bet > balance {
        return Err(AppError::bad_request(
            "bet must be at least $1 and no more than your balance",
        ));
    }
    let id = Uuid::new_v4();
    let view = s
        .blackjack
        .start(user, input.bet, id)
        .await
        .map_err(blackjack_error)?;
    s.bank
        .blackjack_bet(AccountOwner::User(user), id, input.bet)
        .await
        .map_err(AppError::internal)?;
    if view.payout > 0 {
        s.bank
            .blackjack_payout(AccountOwner::User(user), id, view.payout)
            .await
            .map_err(AppError::internal)?;
    }
    s.blackjack.persist().await.map_err(AppError::internal)?;
    Ok(Json(serde_json::json!(view)))
}

pub async fn blackjack_resume(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
) -> Json<Option<crate::blackjack::BlackjackView>> {
    Json(s.blackjack.resume(user).await)
}

#[derive(Deserialize)]
pub struct BlackjackActionRequest {
    pub id: Uuid,
}

pub async fn blackjack_hit(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Json(input): Json<BlackjackActionRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let view = s
        .blackjack
        .hit(user, input.id)
        .await
        .map_err(blackjack_error)?;
    if view.payout > 0 {
        s.bank
            .blackjack_payout(AccountOwner::User(user), input.id, view.payout)
            .await
            .map_err(AppError::internal)?;
    }
    s.blackjack.persist().await.map_err(AppError::internal)?;
    Ok(Json(serde_json::json!(view)))
}

pub async fn blackjack_stand(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Json(input): Json<BlackjackActionRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let view = s
        .blackjack
        .stand(user, input.id)
        .await
        .map_err(blackjack_error)?;
    if view.payout > 0 {
        s.bank
            .blackjack_payout(AccountOwner::User(user), input.id, view.payout)
            .await
            .map_err(AppError::internal)?;
    }
    s.blackjack.persist().await.map_err(AppError::internal)?;
    Ok(Json(serde_json::json!(view)))
}

pub async fn blackjack_double(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Json(input): Json<BlackjackActionRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    match s.blackjack.double(user, input.id).await {
        Ok((view, wager)) => {
            if wager > 0 {
                s.bank
                    .blackjack_bet(AccountOwner::User(user), input.id, wager)
                    .await
                    .map_err(AppError::internal)?;
            }
            if view.payout > 0 {
                s.bank
                    .blackjack_payout(AccountOwner::User(user), input.id, view.payout)
                    .await
                    .map_err(AppError::internal)?;
            }
            s.blackjack.persist().await.map_err(AppError::internal)?;
            Ok(Json(serde_json::json!(view)))
        }
        Err(error) => Err(blackjack_error(error)),
    }
}

pub async fn blackjack_split(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Json(input): Json<BlackjackActionRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    match s.blackjack.split(user, input.id).await {
        Ok((view, wager)) => {
            if wager > 0 {
                s.bank
                    .blackjack_bet(AccountOwner::User(user), input.id, wager)
                    .await
                    .map_err(AppError::internal)?;
            }
            if view.payout > 0 {
                s.bank
                    .blackjack_payout(AccountOwner::User(user), input.id, view.payout)
                    .await
                    .map_err(AppError::internal)?;
            }
            s.blackjack.persist().await.map_err(AppError::internal)?;
            Ok(Json(serde_json::json!(view)))
        }
        Err(error) => Err(blackjack_error(error)),
    }
}

pub async fn blackjack_insurance(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Json(input): Json<BlackjackActionRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    match s.blackjack.insure(user, input.id).await {
        Ok((view, wager)) => {
            s.bank
                .blackjack_bet(AccountOwner::User(user), input.id, wager)
                .await
                .map_err(AppError::internal)?;
            if view.payout > 0 {
                s.bank
                    .blackjack_payout(AccountOwner::User(user), input.id, view.payout)
                    .await
                    .map_err(AppError::internal)?;
            }
            s.blackjack.persist().await.map_err(AppError::internal)?;
            Ok(Json(serde_json::json!(view)))
        }
        Err(error) => Err(blackjack_error(error)),
    }
}

fn blackjack_error(error: BlackjackError) -> AppError {
    match error {
        BlackjackError::NotFound => AppError::not_found("blackjack game not found"),
        BlackjackError::Finished => AppError::bad_request("blackjack game is finished"),
        BlackjackError::ActiveGame => {
            AppError::bad_request("blackjack hand is already in progress")
        }
        BlackjackError::IllegalAction(message) => AppError::bad_request(message),
    }
}

pub async fn hand_blitz(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
) -> Result<Html<String>, AppError> {
    Ok(Html(render::hand_blitz(&s.blitz.stats(user).await)))
}

#[derive(Deserialize)]
pub struct BlitzStartRequest {
    pub difficulty: String,
}

pub async fn hand_blitz_start(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Json(input): Json<BlitzStartRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let difficulty = input
        .difficulty
        .parse::<BlitzDifficulty>()
        .map_err(AppError::bad_request)?;
    let run_id = Uuid::new_v4();
    let config = difficulty.config();
    let run = s
        .blitz
        .start(user, difficulty, run_id)
        .await
        .map_err(|error| match error {
            crate::blitz::BlitzStartError::ActiveRun => {
                AppError::bad_request("hand blitz run is already in progress")
            }
        })?;
    s.bank
        .hand_blitz_buy_in(AccountOwner::User(user), run_id, config.buy_in)
        .await
        .map_err(AppError::internal)?;
    s.blitz.persist_stats().await.map_err(AppError::internal)?;
    Ok(Json(serde_json::json!({
        "run": run,
        "stats": s.blitz.stats(user).await
    })))
}

pub async fn hand_blitz_resume(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
) -> Json<Option<crate::blitz::BlitzRunView>> {
    Json(s.blitz.resume(user).await)
}

#[derive(Deserialize)]
pub struct BlitzAnswerRequest {
    pub run_id: Uuid,
    pub round_id: Uuid,
    pub choice: usize,
}

pub async fn hand_blitz_answer(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Json(input): Json<BlitzAnswerRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let run = s
        .blitz
        .answer(user, input.run_id, input.round_id, input.choice)
        .await
        .map_err(|error| match error {
            BlitzAnswerError::NotFound => AppError::not_found("hand blitz run not found"),
            BlitzAnswerError::Unavailable => {
                AppError::bad_request("hand blitz round is unavailable")
            }
        })?;
    if run.correct {
        s.bank
            .hand_blitz_win(AccountOwner::User(user), input.run_id, run.payout_awarded)
            .await
            .map_err(AppError::internal)?;
    }
    s.blitz.persist_stats().await.map_err(AppError::internal)?;
    Ok(Json(serde_json::json!(run)))
}

/// What the viewer has to spend, which decides what the lobby offers them.
async fn balance_of(state: &AppState, user: Uuid) -> crate::money::Cents {
    state
        .bank
        .account(AccountOwner::User(user))
        .await
        .map_or(0, |account| account.balance)
}

async fn lobby_views(state: &AppState, user: Uuid) -> Vec<LobbyTableView> {
    let mut tables = Vec::new();
    for id in state.tables.ids().await {
        if let Some(table) = state.tables.get(id).await {
            let table = table.lock().await;
            // A finished tournament has nothing left to join or watch.
            if matches!(&table.mode, TableMode::Tournament(state) if state.finished) {
                continue;
            }
            let tournament = match &table.mode {
                TableMode::Tournament(state) => Some(LobbyTournamentView {
                    buy_in: state.config.buy_in,
                    registered: state.registered,
                    seat_count: state.config.seat_count,
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
                buy_in: table.buy_in,
                occupied: table
                    .seats
                    .iter()
                    .filter(|seat| !matches!(seat.occupant, SeatOccupant::Empty))
                    .count(),
                humans: table
                    .seats
                    .iter()
                    .filter(|seat| matches!(seat.occupant, SeatOccupant::Human { .. }))
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
    // Cheapest first, so the ladder reads as a ladder.
    tables.sort_by(|left, right| {
        left.buy_in
            .cmp(&right.buy_in)
            .then_with(|| left.name.cmp(&right.name))
            .then(left.id.cmp(&right.id))
    });
    tables
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
            SeatOccupant::Bot { kind, seat } => {
                Some(AccountOwner::Bot(crate::table::Bot::new(kind, seat)))
            }
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
#[derive(Deserialize)]
pub struct HistoryQuery {
    pub format: Option<String>,
}

/// The table's hand history. Answers JSON when asked for it, so a debugging
/// session can read the raw records without scraping the page.
/// The leaderboard: richest first, and a tie goes to whoever borrowed less.
pub const LEADERBOARD_SIZE: usize = 20;

/// A regular of that kind who is not already sitting at this table.
fn free_bot(table: &Table, kind: BotKind) -> Option<crate::table::Bot> {
    (0..crate::table::Bot::PER_KIND)
        .map(|seat| crate::table::Bot::new(kind, seat))
        .find(|bot| {
            !table
                .seats
                .iter()
                .any(|seat| seat.occupant.as_bot() == Some(*bot))
        })
}

pub async fn leaderboard(AuthUser(_user): AuthUser, State(s): State<AppState>) -> Html<String> {
    let accounts = s.bank.accounts().await;
    let blitz = s.blitz.all_stats().await;
    let poker = s.stats.all().await;
    let mut users: std::collections::HashMap<Uuid, String> = std::collections::HashMap::new();
    for user in s.users.all().await {
        users.insert(user.id, user.display_name);
    }
    let mut ranked: Vec<(crate::bank::AccountOwner, crate::bank::Account)> = accounts
        .into_iter()
        .map(|account| (account.owner.clone(), account))
        .collect();
    // Richest first; a tie goes to whoever borrowed less to get there.
    ranked.sort_by(|(left_owner, left), (right_owner, right)| {
        right
            .balance
            .cmp(&left.balance)
            .then(left.loan_count.cmp(&right.loan_count))
            .then_with(|| format!("{left_owner:?}").cmp(&format!("{right_owner:?}")))
    });
    let rows = ranked
        .into_iter()
        .take(LEADERBOARD_SIZE)
        .enumerate()
        .map(|(index, (owner, account))| {
            let (name, house, blitz_stats) = match &owner {
                crate::bank::AccountOwner::User(id) => (
                    users
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| "Unknown".to_string()),
                    false,
                    blitz.get(id).cloned().unwrap_or_default(),
                ),
                crate::bank::AccountOwner::Bot(bot) => {
                    (bot.name().to_string(), true, Default::default())
                }
            };
            let poker = poker
                .get(&match &owner {
                    crate::bank::AccountOwner::User(id) => format!("user:{id}"),
                    crate::bank::AccountOwner::Bot(bot) => format!("bot:{bot}"),
                })
                .copied()
                .unwrap_or_default();
            crate::view::LeaderboardRow {
                rank: index + 1,
                name,
                house,
                balance: account.balance,
                loan_count: account.loan_count,
                poker,
                blitz: crate::blitz::BlitzDifficulty::ALL
                    .iter()
                    .map(|difficulty| {
                        let tally = blitz_stats.at(*difficulty);
                        crate::view::LeaderboardBlitz {
                            difficulty: difficulty.config().label.to_string(),
                            attempts: tally.attempts,
                            accuracy_percent: tally.accuracy_percent(),
                            best_streak: tally.best_streak,
                        }
                    })
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    Html(render::leaderboard(&rows))
}

pub async fn table_history(
    AuthUser(_user): AuthUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    axum::extract::Query(query): axum::extract::Query<HistoryQuery>,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, AppError> {
    let table = s
        .tables
        .get(id)
        .await
        .ok_or_else(|| AppError::not_found("table not found"))?;
    let (name, hand_no) = {
        let table = table.lock().await;
        (table.name.clone(), table.hand_no)
    };
    let hands = s.history.recent(id, crate::history::HISTORY_PAGE).await;
    let total = s.history.count(id).await;
    let wants_json = query.format.as_deref() == Some("json")
        || headers
            .get(header::ACCEPT)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|accept| accept.contains("application/json"));
    if wants_json {
        return Ok(Json(serde_json::json!({
            "table": id,
            "name": name,
            "hand_no": hand_no,
            "total": total,
            "hands": hands,
        }))
        .into_response());
    }
    let names = {
        let table = table.lock().await;
        seat_names(&s, &table).await
    };
    Ok(Html(render::table_history(id, &name, total, &hands, &names)).into_response())
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
#[serde(deny_unknown_fields)]
pub struct EmptyRequest {}
pub async fn join_table(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(_input): Json<EmptyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let table_arc = s
        .tables
        .get(id)
        .await
        .ok_or_else(|| AppError::not_found("table not found"))?;
    let (no_debt, buy_in, tournament) = {
        let t = table_arc.lock().await;
        let no_debt = matches!(t.mode, TableMode::Cash { no_debt: true });
        (
            no_debt,
            t.buy_in,
            matches!(t.mode, TableMode::Tournament(_)),
        )
    };
    if tournament {
        return Err(AppError::bad_request(
            "tournament registration uses /tournaments/{id}/register",
        ));
    }
    {
        let table = table_arc.lock().await;
        if table
            .seats
            .iter()
            .any(|seat| matches!(seat.occupant, SeatOccupant::Human { user_id } if user_id == user))
        {
            return Err(AppError::bad_request("you are already seated"));
        }
        if crate::cash::seat_for_human(&table.seats).is_none() {
            return Err(AppError::bad_request("table is full"));
        }
    }
    s.bank
        .buy_in(AccountOwner::User(user), id, buy_in, no_debt)
        .await
        .map_err(|_| AppError::bad_request("insufficient funds"))?;
    let mut displaced = None;
    let result = s
        .tables
        .update(id, |t| {
            if t.seats.iter().any(
                |seat| matches!(seat.occupant, SeatOccupant::Human { user_id } if user_id == user),
            ) {
                return Err(anyhow::anyhow!("you are already seated"));
            }
            // A human takes an empty seat, or a house player's if there is none.
            let seat = crate::cash::seat_for_human(&t.seats)
                .ok_or_else(|| anyhow::anyhow!("table is full"))?;
            displaced = t.seats[seat]
                .occupant
                .as_bot()
                .map(|bot| (bot, t.seats[seat].stack));
            t.seats[seat] = crate::table::Seat {
                occupant: SeatOccupant::Human { user_id: user },
                stack: buy_in,
                sitting_out: false,
                pending_departure: false,
            };
            maybe_start_hand(t);
            Ok(())
        })
        .await;
    if let Err(error) = result {
        let _ = s.bank.cash_out(AccountOwner::User(user), id, buy_in).await;
        return Err(AppError::internal(error));
    }
    // A house player who lost their seat takes their chips with them.
    if let Some((bot, stack)) = displaced {
        let _ = s.bank.cash_out(AccountOwner::Bot(bot), id, stack).await;
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
    let mut recorded = Vec::new();
    let (seat, stack, tournament, live_hand) = {
        let mut t = table.lock().await;
        if t.hand.as_ref().is_some_and(|hand| hand.complete) {
            recorded.extend(settle_finished_hand(&mut t));
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
                    hand.fold_seat(seat).map_err(|e| anyhow::anyhow!(e))?;
                }
                if t.hand.as_ref().is_some_and(|hand| hand.complete) {
                    recorded.extend(settle_finished_hand(t));
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
        record_hands(&s, id, &recorded).await;
        crate::driver::settle_pending_departures(&s, id)
            .await
            .map_err(AppError::internal)?;
        let pending = {
            let table = table.lock().await;
            table.seats.iter().any(|seat| {
                matches!(seat.occupant, SeatOccupant::Human { user_id } if user_id == user)
                    && seat.pending_departure
            })
        };
        return Ok(Json(serde_json::json!({"ok":true,"pending":pending})));
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
    record_hands(&s, id, &recorded).await;
    Ok(Json(serde_json::json!({"ok":true})))
}

#[derive(Deserialize)]
pub struct ActionRequest {
    pub kind: String,
    pub amount: Option<i64>,
}
/// Append whatever hands a request settled to the table's history.
async fn record_hands(s: &AppState, id: Uuid, records: &[crate::table::HandRecord]) {
    for record in records {
        if let Err(error) = s.history.append(id, record).await {
            tracing::warn!(%id, %error, "hand history append failed");
        }
        if let Err(error) = s.stats.record(record).await {
            tracing::warn!(%id, %error, "player stats update failed");
        }
    }
}

pub async fn action(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<ActionRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut recorded = Vec::new();
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
            hand.apply_action(action)
                .map_err(|error| anyhow::anyhow!("user action rejected: {error}"))?;
            let complete = hand.complete;
            if complete {
                recorded.extend(settle_finished_hand(t));
            }
            Ok(())
        })
        .await
        .map_err(|error| {
            let message = error.to_string();
            if message.contains("user action rejected")
                || message == "not your turn"
                || message == "no hand in progress"
                || message == "you are not seated"
                || message == "amount required"
                || message == "unknown action"
            {
                AppError::bad_request(message)
            } else {
                AppError::internal(message)
            }
        })?;
    record_hands(&s, id, &recorded).await;
    Ok(Json(serde_json::json!({"ok":true})))
}

/// Ask the house to play one hand at a table nobody is sitting at. Only a
/// signed-in watcher may, and only when there is nothing already running.
pub async fn deal_bot_hand(
    AuthUser(_user): AuthUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    s.tables
        .update(id, |table| {
            if table.hand.is_some() {
                return Err(anyhow::anyhow!("a hand is already in progress"));
            }
            if !table.waits_for_a_watcher() {
                return Err(anyhow::anyhow!("this table has players of its own"));
            }
            // One click, one hand; clicking again while it plays changes nothing.
            table.bot_hands_requested = 1;
            maybe_start_hand(table);
            Ok(())
        })
        .await
        .map_err(|error| AppError::bad_request(error.to_string()))?;
    Ok(Json(serde_json::json!({"ok":true})))
}

pub async fn continue_table(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    s.tables
        .update(id, |table| {
            let seated = table.seats.iter().any(
                |seat| matches!(seat.occupant, SeatOccupant::Human { user_id } if user_id == user),
            );
            if !seated {
                return Err(anyhow::anyhow!("you are not seated"));
            }
            if table.hand.is_some() || table.last_hand.is_none() {
                return Err(anyhow::anyhow!("no showdown to continue"));
            }
            // The runout occupies the first stretch of the pause and cannot be
            // cut short; only the time to read the result may be skipped.
            let runout =
                chrono::Duration::seconds(crate::table::runout_seconds(table.last_hand.as_ref()));
            let pause = chrono::Duration::seconds(crate::table::result_pause_seconds(
                table.last_hand.as_ref(),
            ));
            let earliest = table
                .next_action_at
                .map(|at| at - pause + runout)
                .unwrap_or_else(Utc::now);
            table.next_action_at = Some(earliest.max(Utc::now()));
            Ok(())
        })
        .await
        .map_err(|error| AppError::bad_request(error.to_string()))?;
    Ok(Json(serde_json::json!({"ok":true})))
}

#[derive(Deserialize)]
pub struct RebuyRequest {}
pub async fn rebuy_table(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(_input): Json<RebuyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let table = s
        .tables
        .get(id)
        .await
        .ok_or_else(|| AppError::not_found("table not found"))?;
    let (no_debt, buy_in, tournament) = {
        let table = table.lock().await;
        (
            matches!(table.mode, TableMode::Cash { no_debt: true }),
            table.buy_in,
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
    s.bank
        .buy_in(AccountOwner::User(user), id, buy_in, no_debt)
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
            seat.stack += buy_in;
            Ok(())
        })
        .await;
    if let Err(error) = result {
        let _ = s.bank.cash_out(AccountOwner::User(user), id, buy_in).await;
        return Err(AppError::internal(error));
    }
    Ok(Json(serde_json::json!({"ok":true})))
}

#[derive(Deserialize)]
pub struct BotRequest {
    pub seat: usize,
    pub kind: Option<String>,
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
    let (old, buy_in, no_debt, started, live_hand) = {
        let table = table.lock().await;
        let seat = table
            .seats
            .get(input.seat)
            .ok_or_else(|| AppError::bad_request("invalid seat"))?;
        (
            seat.occupant.clone(),
            table.buy_in,
            matches!(table.mode, TableMode::Cash { no_debt: true }),
            matches!(&table.mode, TableMode::Tournament(state) if state.started || state.finished),
            table.hand.is_some(),
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
    if !matches!(old, SeatOccupant::Empty) && matches!(old, SeatOccupant::Human { .. }) {
        return Err(AppError::bad_request("cannot replace a human seat"));
    }
    if live_hand && !matches!(old, SeatOccupant::Empty) {
        return Err(AppError::bad_request(
            "bot seating is unavailable while a hand is in progress",
        ));
    }
    if let Some(bot) = old.as_bot() {
        let stack = s.tables.get(id).await.unwrap().lock().await.seats[input.seat].stack;
        if !tournament {
            s.bank
                .cash_out(AccountOwner::Bot(bot), id, stack)
                .await
                .map_err(AppError::internal)?;
        }
    }
    let mut bought = None;
    if let Some(kind_name) = input.kind {
        let kind = kind_name
            .parse::<BotKind>()
            .map_err(AppError::bad_request)?;
        // Seat one of that kind's regulars who is not already at this table.
        let bot = {
            let table = s
                .tables
                .get(id)
                .await
                .ok_or_else(|| AppError::not_found("table not found"))?;
            let table = table.lock().await;
            free_bot(&table, kind)
                .ok_or_else(|| AppError::bad_request("every one of them is already seated"))?
        };
        let amount = if tournament {
            let table = s
                .tables
                .get(id)
                .await
                .ok_or_else(|| AppError::not_found("table not found"))?;
            let table = table.lock().await;
            match &table.mode {
                TableMode::Tournament(state) => state.config.buy_in,
                TableMode::Cash { .. } => buy_in,
            }
        } else {
            buy_in
        };
        s.bank
            .buy_in(
                AccountOwner::Bot(bot),
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
                    no_debt
                },
            )
            .await
            .map_err(|_| AppError::bad_request("insufficient funds"))?;
        bought = Some((bot, amount));
    }
    let result = s
        .tables
        .update(id, |table| {
            let seat = table
                .seats
                .get_mut(input.seat)
                .ok_or_else(|| anyhow::anyhow!("invalid seat"))?;
            if let Some((bot, amount)) = bought {
                seat.occupant = SeatOccupant::bot(bot);
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
        if let Some((bot, amount)) = bought {
            let _ = s.bank.cash_out(AccountOwner::Bot(bot), id, amount).await;
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

pub async fn bank_re_up(
    AuthUser(user): AuthUser,
    State(s): State<AppState>,
    Json(_input): Json<EmptyRequest>,
) -> Result<Json<crate::bank::Account>, AppError> {
    s.bank
        .re_up(AccountOwner::User(user))
        .await
        .map(Json)
        .map_err(|_| AppError::bad_request("re-up is only available below $100"))
}
