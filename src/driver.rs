use crate::{
    app::AppState,
    bank::{AccountOwner, LedgerKind},
    table::{SeatOccupant, maybe_start_hand, settle_finished_hand},
    view::hand_view,
};
use chrono::{DateTime, Duration, Utc};
use tokio::time::{Duration as TokioDuration, interval};

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = interval(TokioDuration::from_millis(250));
        loop {
            ticker.tick().await;
            if let Err(error) = tick_once(&state).await {
                tracing::warn!(%error, "table driver tick failed");
            }
        }
    });
}

pub async fn tick_once(state: &AppState) -> Result<(), anyhow::Error> {
    tick_once_at(state, Utc::now()).await
}

pub async fn tick_once_at(state: &AppState, now: DateTime<Utc>) -> Result<(), anyhow::Error> {
    state.blitz.expire(now).await;
    let mut ids = state.tables.ids().await;
    ids.sort();
    for id in ids {
        if state.tables.get(id).await.is_none() {
            continue;
        }
        let mut recorded = Vec::new();
        if let Err(error) = state
            .tables
            .update(id, |table| {
                if table.hand.as_ref().is_some_and(|hand| hand.complete) {
                    recorded.extend(settle_finished_hand(table));
                }
                if table.hand.is_none() {
                    if table.next_action_at.is_none_or(|at| at <= now) {
                        maybe_start_hand(table);
                    }
                    return Ok(());
                }
                let Some(hand) = table.hand.as_mut() else {
                    return Ok(());
                };
                let Some(seat) = hand.current_player else {
                    return Ok(());
                };
                let Some(bot) = table
                    .seats
                    .get(seat)
                    .and_then(|value| value.occupant.as_bot())
                else {
                    return Ok(());
                };
                if table.next_action_at.is_none() {
                    table.next_action_at = Some(now + Duration::milliseconds(400));
                    return Ok(());
                }
                if table.next_action_at.is_some_and(|at| at > now) {
                    return Ok(());
                }
                let kind = bot.kind;
                let view = hand_view(hand, Some(seat));
                let legal = view
                    .legal_actions
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("bot turn has no legal actions"))?;
                let action = kind.act(
                    &view,
                    &legal,
                    hand.seed.wrapping_add(hand.players.len() as u64),
                );
                hand.apply_action(action)
                    .map_err(|error| anyhow::anyhow!(error))?;
                table.next_action_at = None;
                if hand.complete {
                    recorded.extend(settle_finished_hand(table));
                }
                Ok(())
            })
            .await
        {
            tracing::warn!(%id, %error, "table driver update failed");
            continue;
        }
        for record in &recorded {
            if let Err(error) = state.history.append(id, record).await {
                tracing::warn!(%id, %error, "hand history append failed");
            }
            if let Err(error) = state.stats.record(record).await {
                tracing::warn!(%id, %error, "player stats update failed");
            }
        }
        if let Err(error) = settle_pending_departures(state, id).await {
            tracing::warn!(%id, %error, "pending departure settlement failed");
            continue;
        }
        if let Err(error) = bank_bot_profits(state, id).await {
            tracing::warn!(%id, %error, "banking house profits failed");
            continue;
        }
        if let Err(error) = rebuy_busted_cash_bots(state, id).await {
            tracing::warn!(%id, %error, "cash bot rebuy failed");
            continue;
        }
        if let Err(error) = pay_tournament_if_finished(state, id).await {
            tracing::warn!(%id, %error, "tournament payout failed");
        }
        if let Err(error) = seat_a_house_player(state, id).await {
            tracing::warn!(%id, %error, "house seating failed");
        }
    }
    Ok(())
}

/// Cash tables built by hand before the ladder existed have nowhere to belong,
/// so they are paid out and retired at startup. Tournaments are still made by
/// players and are left alone.
pub async fn retire_custom_cash_tables(state: &AppState) -> Result<(), anyhow::Error> {
    for id in state.tables.ids().await {
        let Some(table) = state.tables.get(id).await else {
            continue;
        };
        let stacks = {
            let table = table.lock().await;
            if table.cash_tier.is_some()
                || !matches!(table.mode, crate::table::TableMode::Cash { .. })
            {
                continue;
            }
            table
                .seats
                .iter()
                .filter(|seat| seat.stack > 0)
                .map(|seat| (seat.occupant.clone(), seat.stack))
                .collect::<Vec<_>>()
        };
        // Nobody loses chips to the clear-out.
        for (occupant, stack) in stacks {
            let owner = match occupant {
                SeatOccupant::Human { user_id } => AccountOwner::User(user_id),
                SeatOccupant::Bot { kind, seat } => {
                    AccountOwner::Bot(crate::table::Bot::new(kind, seat))
                }
                SeatOccupant::Empty => continue,
            };
            if let Err(error) = state.bank.cash_out(owner, id, stack).await {
                tracing::warn!(%id, %error, "cashing out a retired table failed");
            }
        }
        state.tables.remove(id).await?;
        tracing::info!(%id, "retired a custom cash table");
    }
    Ok(())
}

/// The standing cash tables always exist. Called once at startup; the tick
/// keeps their seats full from there.
pub async fn ensure_cash_ladder(state: &AppState) -> Result<(), anyhow::Error> {
    let mut present = std::collections::BTreeSet::new();
    for id in state.tables.ids().await {
        if let Some(table) = state.tables.get(id).await
            && let Some(tier) = table.lock().await.cash_tier
        {
            present.insert(tier);
        }
    }
    for tier in 0..crate::cash::TIERS.len() {
        if !present.contains(&tier) {
            state.tables.insert(crate::cash::table(tier)).await?;
        }
    }
    Ok(())
}

/// Fill one empty seat at a standing table from the roster. One per tick is
/// enough: a table refills in a couple of seconds and nothing stalls the loop.
async fn seat_a_house_player(state: &AppState, id: uuid::Uuid) -> Result<(), anyhow::Error> {
    let Some(table) = state.tables.get(id).await else {
        return Ok(());
    };
    let (buy_in, vacancy, bot) = {
        let table = table.lock().await;
        let Some(tier) = table.cash_tier else {
            return Ok(());
        };
        // Never rearrange a table mid-hand.
        if table.hand.is_some() {
            return Ok(());
        }
        let Some(vacancy) = table
            .seats
            .iter()
            .position(|seat| matches!(seat.occupant, SeatOccupant::Empty))
        else {
            return Ok(());
        };
        let Some(bot) = crate::cash::house_bot(&table, tier, vacancy) else {
            return Ok(());
        };
        (table.buy_in, vacancy, bot)
    };
    if state
        .bank
        .buy_in(AccountOwner::Bot(bot), id, buy_in, false)
        .await
        .is_err()
    {
        return Ok(());
    }
    if let Err(error) = state
        .tables
        .update(id, |table| {
            let value = table
                .seats
                .get_mut(vacancy)
                .ok_or_else(|| anyhow::anyhow!("seat missing"))?;
            if matches!(value.occupant, SeatOccupant::Empty) {
                value.occupant = SeatOccupant::bot(bot);
                value.stack = buy_in;
                value.sitting_out = false;
                value.pending_departure = false;
            }
            maybe_start_hand(table);
            Ok(())
        })
        .await
    {
        let _ = state
            .bank
            .cash_out(AccountOwner::Bot(bot), id, buy_in)
            .await;
        tracing::warn!(%id, %error, "seating a house player failed");
    }
    Ok(())
}

pub(crate) async fn settle_pending_departures(
    state: &AppState,
    id: uuid::Uuid,
) -> Result<(), anyhow::Error> {
    let table = state
        .tables
        .get(id)
        .await
        .ok_or_else(|| anyhow::anyhow!("table missing"))?;
    let departures = {
        let table = table.lock().await;
        if table.hand.is_some() {
            return Ok(());
        }
        table
            .seats
            .iter()
            .enumerate()
            .filter(|(_, seat)| seat.pending_departure)
            .map(|(index, seat)| {
                (
                    index,
                    seat.occupant.clone(),
                    seat.stack,
                    matches!(table.mode, crate::table::TableMode::Tournament(_)),
                )
            })
            .collect::<Vec<_>>()
    };
    for (seat, occupant, stack, tournament) in departures {
        if !tournament {
            if let SeatOccupant::Human { user_id } = occupant {
                state
                    .bank
                    .cash_out(AccountOwner::User(user_id), id, stack)
                    .await?;
            } else if let Some(bot) = occupant.as_bot()
                && stack > 0
            {
                state
                    .bank
                    .cash_out(AccountOwner::Bot(bot), id, stack)
                    .await?;
            }
        } else {
            state
                .tables
                .update(id, |table| {
                    if let crate::table::TableMode::Tournament(state) = &mut table.mode
                        && !state.finish_order.contains(&seat)
                    {
                        state.finish_order.push(seat);
                    }
                    Ok(())
                })
                .await?;
        }
        state
            .tables
            .update(id, |table| {
                let seat = table
                    .seats
                    .get_mut(seat)
                    .ok_or_else(|| anyhow::anyhow!("seat missing"))?;
                seat.occupant = SeatOccupant::Empty;
                seat.stack = 0;
                seat.sitting_out = false;
                seat.pending_departure = false;
                if let crate::table::TableMode::Tournament(state) = &mut table.mode {
                    let alive = table
                        .seats
                        .iter()
                        .filter(|seat| {
                            !matches!(seat.occupant, SeatOccupant::Empty) && seat.stack > 0
                        })
                        .count();
                    if alive <= 1 {
                        state.finished = true;
                    }
                }
                Ok(())
            })
            .await?;
    }
    Ok(())
}

/// A house player who has doubled up takes their original buy-in off the table
/// and banks it, leaving the winnings in play. Otherwise the whole roll rides
/// on one seat forever and the ladder's stakes stop meaning anything.
async fn bank_bot_profits(state: &AppState, id: uuid::Uuid) -> Result<(), anyhow::Error> {
    let table = state
        .tables
        .get(id)
        .await
        .ok_or_else(|| anyhow::anyhow!("table missing"))?;
    let cashes = {
        let table = table.lock().await;
        if !matches!(table.mode, crate::table::TableMode::Cash { .. }) || table.hand.is_some() {
            return Ok(());
        }
        let buy_in = table.buy_in;
        table
            .seats
            .iter()
            .enumerate()
            .filter_map(|(seat, value)| {
                let bot = value.occupant.as_bot()?;
                (value.stack >= buy_in * 2).then_some((seat, bot, buy_in))
            })
            .collect::<Vec<_>>()
    };
    for (seat, bot, amount) in cashes {
        state
            .tables
            .update(id, |table| {
                let value = table
                    .seats
                    .get_mut(seat)
                    .ok_or_else(|| anyhow::anyhow!("seat missing"))?;
                if value.occupant.as_bot() == Some(bot) && value.stack >= amount * 2 {
                    value.stack -= amount;
                }
                Ok(())
            })
            .await?;
        if let Err(error) = state
            .bank
            .cash_out(AccountOwner::Bot(bot), id, amount)
            .await
        {
            tracing::warn!(%id, %error, "banking a house player's profit failed");
        }
    }
    Ok(())
}

async fn rebuy_busted_cash_bots(state: &AppState, id: uuid::Uuid) -> Result<(), anyhow::Error> {
    let table = state
        .tables
        .get(id)
        .await
        .ok_or_else(|| anyhow::anyhow!("table missing"))?;
    let rebuys = {
        let table = table.lock().await;
        if !matches!(table.mode, crate::table::TableMode::Cash { .. }) {
            return Ok(());
        }
        table
            .seats
            .iter()
            .enumerate()
            .filter_map(|(seat, value)| {
                (value.stack == 0
                    && !value.sitting_out
                    && matches!(&value.occupant, SeatOccupant::Bot { .. }))
                .then_some((seat, value.occupant.clone(), table.buy_in))
            })
            .collect::<Vec<_>>()
    };
    for (seat, occupant, amount) in rebuys {
        let Some(bot) = occupant.as_bot() else {
            continue;
        };
        state
            .bank
            .buy_in(AccountOwner::Bot(bot), id, amount, false)
            .await?;
        if let Err(error) = state
            .tables
            .update(id, |table| {
                let value = table
                    .seats
                    .get_mut(seat)
                    .ok_or_else(|| anyhow::anyhow!("seat missing"))?;
                if value.occupant.as_bot() == Some(bot) && value.stack == 0 {
                    value.stack = amount;
                }
                Ok(())
            })
            .await
        {
            let _ = state
                .bank
                .cash_out(AccountOwner::Bot(bot), id, amount)
                .await;
            return Err(error);
        }
    }
    Ok(())
}

async fn pay_tournament_if_finished(state: &AppState, id: uuid::Uuid) -> Result<(), anyhow::Error> {
    let table = state
        .tables
        .get(id)
        .await
        .ok_or_else(|| anyhow::anyhow!("table missing"))?;
    let (order, payouts, prize_pool) = {
        let table = table.lock().await;
        let crate::table::TableMode::Tournament(tournament) = &table.mode else {
            return Ok(());
        };
        if !tournament.finished || tournament.paid_out {
            return Ok(());
        }
        let mut winners = Vec::new();
        for seat in (0..table.seats.len()).rev() {
            if table.seats[seat].stack > 0
                && !matches!(table.seats[seat].occupant, SeatOccupant::Empty)
                && !tournament.finish_order.contains(&seat)
            {
                winners.push(seat);
            }
        }
        let mut order = winners;
        order.extend(tournament.finish_order.iter().rev().copied());
        (
            order,
            tournament.config.payout_percentages.clone(),
            tournament.prize_pool,
        )
    };
    let table = state
        .tables
        .get(id)
        .await
        .ok_or_else(|| anyhow::anyhow!("table missing"))?;
    let owners = {
        let table = table.lock().await;
        order
            .iter()
            .enumerate()
            .filter_map(|(position, seat)| {
                let occupant = table.seats.get(*seat)?.occupant.clone();
                let owner = match occupant {
                    SeatOccupant::Human { user_id } => AccountOwner::User(user_id),
                    SeatOccupant::Bot { kind, seat } => {
                        AccountOwner::Bot(crate::table::Bot::new(kind, seat))
                    }
                    SeatOccupant::Empty => return None,
                };
                Some((position, owner))
            })
            .collect::<Vec<_>>()
    };
    let mut paid = 0;
    let total = owners.len();
    for (position, owner) in owners {
        let percent = payouts.get(position).copied().unwrap_or(0);
        let amount = if position + 1 == total {
            prize_pool - paid
        } else {
            prize_pool * i64::from(percent) / 100
        };
        if amount == 0 {
            continue;
        }
        paid += amount;
        state
            .bank
            .append(
                owner,
                LedgerKind::TournamentPrize { tournament: id },
                amount,
                format!("tournament place {}", position + 1),
            )
            .await?;
    }
    state
        .tables
        .update(id, |table| {
            if let crate::table::TableMode::Tournament(tournament) = &mut table.mode {
                tournament.paid_out = true;
            }
            Ok(())
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{AppState, build_webauthn},
        bank::{AccountOwner, BankStore},
        blitz::BlitzStore,
        store::TableStore,
        table::{
            BlindLevel, BotKind, Seat, Stakes, Table, TableMode, TournamentConfig, TournamentState,
        },
        users::UserStore,
    };
    use axum_extra::extract::cookie::Key;
    use std::sync::Arc;
    use uuid::Uuid;

    #[tokio::test]
    async fn a_doubled_up_house_player_banks_the_buy_in() {
        let root = std::env::temp_dir().join(format!("two-seven-profit-{}", Uuid::new_v4()));
        let bank = BankStore::load(&root).await.unwrap();
        let blitz = BlitzStore::load(&root).await.unwrap();
        let tables = TableStore::load(&root).await.unwrap();
        let users = Arc::new(UserStore::load(&root).await.unwrap());
        let state = AppState {
            users,
            bank: bank.clone(),
            blackjack: crate::blackjack::BlackjackStore::new(),
            blitz,
            tables: tables.clone(),
            history: crate::history::HistoryStore::load(&root).await.unwrap(),
            stats: crate::stats::StatsStore::load(&root).await.unwrap(),
            admin_password: Arc::new("test-admin-password".into()),
            webauthn: Arc::new(build_webauthn().unwrap()),
            key: Key::generate(),
            passkey_disabled: true,
        };
        let bot = crate::table::Bot::new(BotKind::Shark, 0);
        let mut table = Table::new(
            "profits".into(),
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            TableMode::Cash { no_debt: false },
            2,
            20_000,
        );
        table.seats[0] = crate::table::Seat {
            occupant: SeatOccupant::bot(bot),
            // Doubled up, and then some.
            stack: 45_000,
            sitting_out: false,
            pending_departure: false,
        };
        table.seats[1] = crate::table::Seat {
            occupant: SeatOccupant::bot(crate::table::Bot::new(BotKind::Rock, 0)),
            stack: 19_000,
            sitting_out: false,
            pending_departure: false,
        };
        let id = tables.insert(table).await.unwrap();

        bank_bot_profits(&state, id).await.unwrap();

        let table = tables.get(id).await.unwrap();
        let table = table.lock().await;
        assert_eq!(
            table.seats[0].stack, 25_000,
            "the original buy-in comes off the table"
        );
        assert_eq!(
            table.seats[1].stack, 19_000,
            "a player still under a double-up is left alone"
        );
        let account = bank.account(AccountOwner::Bot(bot)).await.unwrap();
        assert_eq!(account.balance, 20_000, "and lands in their account");
    }

    /// A table with no people at it deals only on request, so a test that
    /// wants the house to keep playing has to keep asking.
    async fn keep_dealing(tables: &TableStore, id: Uuid) {
        let _ = tables
            .update(id, |table| {
                table.bot_hands_requested = 1;
                Ok(())
            })
            .await;
    }

    #[tokio::test]
    async fn the_standing_tables_seed_themselves_and_fill_with_the_house() {
        let root = std::env::temp_dir().join(format!("two-seven-ladder-{}", Uuid::new_v4()));
        let bank = BankStore::load(&root).await.unwrap();
        let blitz = BlitzStore::load(&root).await.unwrap();
        let tables = TableStore::load(&root).await.unwrap();
        let users = Arc::new(UserStore::load(&root).await.unwrap());
        let history = crate::history::HistoryStore::load(&root).await.unwrap();
        let stats = crate::stats::StatsStore::load(&root).await.unwrap();
        let state = AppState {
            users,
            bank,
            blackjack: crate::blackjack::BlackjackStore::new(),
            blitz,
            tables: tables.clone(),
            history,
            stats,
            admin_password: Arc::new("test-admin-password".into()),
            webauthn: Arc::new(build_webauthn().unwrap()),
            key: Key::generate(),
            passkey_disabled: true,
        };
        ensure_cash_ladder(&state).await.unwrap();
        assert_eq!(tables.ids().await.len(), crate::cash::TIERS.len());
        // Seeding twice must not double the ladder.
        ensure_cash_ladder(&state).await.unwrap();
        assert_eq!(tables.ids().await.len(), crate::cash::TIERS.len());

        // One seat fills per tick, so a table is full in a handful of them.
        for _ in 0..(crate::cash::SEATS * crate::cash::TIERS.len() + 4) {
            tick_once(&state).await.unwrap();
        }
        let cheapest = {
            let mut found = None;
            for id in tables.ids().await {
                let table = tables.get(id).await.unwrap();
                let table = table.lock().await;
                if table.cash_tier == Some(0) {
                    found = Some(table.clone());
                }
            }
            found.expect("the cheapest table")
        };
        assert_eq!(cheapest.buy_in, crate::cash::TIERS[0]);
        assert_eq!(cheapest.max_seats, crate::cash::SEATS);
        let seated: Vec<_> = cheapest
            .seats
            .iter()
            .filter_map(|seat| seat.occupant.as_bot())
            .collect();
        assert_eq!(
            seated.len(),
            crate::cash::SEATS,
            "the house fills the table"
        );
        let distinct: std::collections::BTreeSet<_> = seated.iter().collect();
        assert_eq!(distinct.len(), seated.len(), "nobody sits down twice");
        assert!(
            seated
                .iter()
                .filter(|bot| bot.kind == crate::table::BotKind::Fish)
                .count()
                >= 3,
            "the cheapest table is mostly fish: {seated:?}"
        );
        // Each of them bought in from their own account.
        for bot in &seated {
            let account = state.bank.account(AccountOwner::Bot(*bot)).await.unwrap();
            assert!(
                account.entries.iter().any(|entry| entry.delta < 0),
                "{} paid their own way",
                bot.name()
            );
        }
    }

    #[tokio::test]
    async fn cash_bots_rebuy_after_busting() {
        let root = std::env::temp_dir().join(format!("two-seven-rebuy-{}", Uuid::new_v4()));
        let bank = BankStore::load(&root).await.unwrap();
        let blitz = BlitzStore::load(&root).await.unwrap();
        let tables = TableStore::load(&root).await.unwrap();
        let users = Arc::new(UserStore::load(&root).await.unwrap());
        let history = crate::history::HistoryStore::load(&root).await.unwrap();
        let stats = crate::stats::StatsStore::load(&root).await.unwrap();
        let state = AppState {
            users,
            bank: bank.clone(),
            blackjack: crate::blackjack::BlackjackStore::new(),
            blitz,
            tables: tables.clone(),
            history,
            stats,
            admin_password: Arc::new("test-admin-password".into()),
            webauthn: Arc::new(build_webauthn().unwrap()),
            key: Key::generate(),
            passkey_disabled: true,
        };
        let mut table = Table::new(
            "rebuy".into(),
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            TableMode::Cash { no_debt: false },
            2,
            100,
        );
        table.seats[0] = Seat {
            occupant: SeatOccupant::bot(crate::table::Bot::new(BotKind::Rock, 0)),
            stack: 0,
            sitting_out: false,
            pending_departure: false,
        };
        table.seats[1] = Seat {
            occupant: SeatOccupant::bot(crate::table::Bot::new(BotKind::Fish, 0)),
            stack: 100,
            sitting_out: false,
            pending_departure: false,
        };
        bank.buy_in(
            AccountOwner::Bot(crate::table::Bot::new(BotKind::Fish, 0)),
            table.id,
            100,
            false,
        )
        .await
        .unwrap();
        let id = tables.insert(table).await.unwrap();
        tick_once_at(&state, Utc::now() + Duration::seconds(1))
            .await
            .unwrap();
        let table = tables.get(id).await.unwrap();
        let table = table.lock().await;
        assert_eq!(table.seats[0].stack, 100);
        assert!(
            bank.account(AccountOwner::Bot(crate::table::Bot::new(BotKind::Rock, 0)))
                .await
                .unwrap()
                .entries
                .iter()
                .any(|entry| entry.delta == -100)
        );
    }

    #[tokio::test]
    async fn blitz_expiry_sweep_closes_run_and_allows_restart() {
        let root = std::env::temp_dir().join(format!("two-seven-blitz-expiry-{}", Uuid::new_v4()));
        let bank = BankStore::load(&root).await.unwrap();
        let blitz = BlitzStore::load(&root).await.unwrap();
        let tables = TableStore::load(&root).await.unwrap();
        let users = Arc::new(UserStore::load(&root).await.unwrap());
        let user = Uuid::new_v4();
        let state = AppState {
            users,
            bank,
            blackjack: crate::blackjack::BlackjackStore::load(&root).await.unwrap(),
            blitz: blitz.clone(),
            tables,
            history: crate::history::HistoryStore::load(&root).await.unwrap(),
            stats: crate::stats::StatsStore::load(&root).await.unwrap(),
            admin_password: Arc::new("test-admin-password".into()),
            webauthn: Arc::new(build_webauthn().unwrap()),
            key: Key::generate(),
            passkey_disabled: true,
        };
        blitz
            .start(user, crate::blitz::BlitzDifficulty::Easy, Uuid::new_v4())
            .await
            .unwrap();

        tick_once_at(&state, Utc::now() + Duration::seconds(30))
            .await
            .unwrap();
        assert!(blitz.resume(user).await.is_none());
        assert!(
            blitz
                .start(user, crate::blitz::BlitzDifficulty::Easy, Uuid::new_v4())
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn four_bots_complete_hands_without_losing_chips() {
        let root = std::env::temp_dir().join(format!("two-seven-driver-{}", Uuid::new_v4()));
        let bank = BankStore::load(&root).await.unwrap();
        let blitz = BlitzStore::load(&root).await.unwrap();
        let tables = TableStore::load(&root).await.unwrap();
        let users = Arc::new(UserStore::load(&root).await.unwrap());
        let history = crate::history::HistoryStore::load(&root).await.unwrap();
        let stats = crate::stats::StatsStore::load(&root).await.unwrap();
        let state = AppState {
            users,
            bank: bank.clone(),
            blackjack: crate::blackjack::BlackjackStore::new(),
            blitz,
            tables: tables.clone(),
            history,
            stats,
            admin_password: Arc::new("test-admin-password".into()),
            webauthn: Arc::new(build_webauthn().unwrap()),
            key: Key::generate(),
            passkey_disabled: true,
        };
        let kinds = [
            BotKind::Fish,
            BotKind::Rock,
            BotKind::Grinder,
            BotKind::Shark,
        ];
        let mut table = Table::new(
            "bots".into(),
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            TableMode::Cash { no_debt: false },
            4,
            100,
        );
        for (seat, kind) in kinds.into_iter().enumerate() {
            bank.buy_in(
                AccountOwner::Bot(crate::table::Bot::new(kind, 0)),
                table.id,
                100,
                false,
            )
            .await
            .unwrap();
            table.seats[seat] = Seat {
                occupant: crate::table::SeatOccupant::bot(crate::table::Bot::new(kind, 0)),
                stack: 100,
                sitting_out: false,
                pending_departure: false,
            };
        }
        let initial_table_chips = table.seats.iter().map(|seat| seat.stack).sum::<i64>();
        let mut initial_bank_balances = Vec::new();
        for kind in kinds.iter().copied() {
            initial_bank_balances.push(
                bank.account(AccountOwner::Bot(crate::table::Bot::new(kind, 0)))
                    .await
                    .unwrap()
                    .balance,
            );
        }
        let initial_chips = initial_table_chips + initial_bank_balances.iter().sum::<i64>();
        let id = tables.insert(table).await.unwrap();
        let mut now = Utc::now();
        let mut aggressive_actions = std::collections::HashSet::new();
        for _ in 0..2_000 {
            now += Duration::seconds(1);
            // Nobody is sitting down, so every hand has to be asked for.
            keep_dealing(&tables, id).await;
            tick_once_at(&state, now).await.unwrap();
            let table = tables.get(id).await.unwrap();
            let table = table.lock().await;
            if let Some(hand) = &table.hand {
                for (event_index, event) in hand.events.iter().enumerate() {
                    if matches!(
                        event.kind,
                        crate::holdem::HandEventKind::Bet
                            | crate::holdem::HandEventKind::Raise
                            | crate::holdem::HandEventKind::AllIn
                    ) {
                        aggressive_actions.insert((hand.seed, event_index));
                    }
                }
            }
        }
        let table = tables.get(id).await.unwrap();
        let table = table.lock().await;
        assert!(table.hand_no > 5);
        assert!(
            aggressive_actions.len() >= 4,
            "mixed bots should visibly wager across completed hands: {aggressive_actions:?}"
        );
        let table_chips = table.seats.iter().map(|seat| seat.stack).sum::<i64>();
        drop(table);
        let mut bank_chips = 0;
        for kind in kinds {
            let account = bank
                .account(AccountOwner::Bot(crate::table::Bot::new(kind, 0)))
                .await
                .unwrap();
            bank_chips += account.balance;
            assert_eq!(
                account.entries.iter().map(|entry| entry.delta).sum::<i64>(),
                account.balance
            );
        }
        assert_eq!(
            table_chips + bank_chips,
            initial_chips,
            "cash bot chips must be conserved across table and bot bank accounts: initial={initial_chips}, table={table_chips}, bank={bank_chips}"
        );
    }

    #[tokio::test]
    async fn four_bots_complete_limit_hands_without_losing_chips() {
        let root = std::env::temp_dir().join(format!("two-seven-limit-driver-{}", Uuid::new_v4()));
        let bank = BankStore::load(&root).await.unwrap();
        let blitz = BlitzStore::load(&root).await.unwrap();
        let tables = TableStore::load(&root).await.unwrap();
        let users = Arc::new(UserStore::load(&root).await.unwrap());
        let history = crate::history::HistoryStore::load(&root).await.unwrap();
        let stats = crate::stats::StatsStore::load(&root).await.unwrap();
        let state = AppState {
            users,
            bank: bank.clone(),
            blackjack: crate::blackjack::BlackjackStore::new(),
            blitz,
            tables: tables.clone(),
            history,
            stats,
            admin_password: Arc::new("test-admin-password".into()),
            webauthn: Arc::new(build_webauthn().unwrap()),
            key: Key::generate(),
            passkey_disabled: true,
        };
        let kinds = [
            BotKind::Fish,
            BotKind::Rock,
            BotKind::Grinder,
            BotKind::Shark,
        ];
        let mut table = Table::new(
            "limit bots".into(),
            Stakes::Limit {
                small_bet: 2,
                big_bet: 4,
            },
            TableMode::Cash { no_debt: false },
            4,
            100,
        );
        for (seat, kind) in kinds.into_iter().enumerate() {
            bank.buy_in(
                AccountOwner::Bot(crate::table::Bot::new(kind, 0)),
                table.id,
                100,
                false,
            )
            .await
            .unwrap();
            table.seats[seat] = Seat {
                occupant: SeatOccupant::bot(crate::table::Bot::new(kind, 0)),
                stack: 100,
                sitting_out: false,
                pending_departure: false,
            };
        }
        let initial_table_chips = table.seats.iter().map(|seat| seat.stack).sum::<i64>();
        let mut initial_bank_chips = 0;
        for kind in kinds {
            initial_bank_chips += bank
                .account(AccountOwner::Bot(crate::table::Bot::new(kind, 0)))
                .await
                .unwrap()
                .balance;
        }
        let initial_chips = initial_table_chips + initial_bank_chips;
        let id = tables.insert(table).await.unwrap();
        let mut now = Utc::now();
        for _ in 0..4_000 {
            now += Duration::seconds(1);
            keep_dealing(&tables, id).await;
            tick_once_at(&state, now).await.unwrap();
        }
        let table = tables.get(id).await.unwrap();
        let table = table.lock().await;
        assert!(table.hand_no > 3);
        let table_chips = table.seats.iter().map(|seat| seat.stack).sum::<i64>();
        drop(table);
        let mut bank_chips = 0;
        for kind in kinds {
            bank_chips += bank
                .account(AccountOwner::Bot(crate::table::Bot::new(kind, 0)))
                .await
                .unwrap()
                .balance;
        }
        assert_eq!(
            table_chips + bank_chips,
            initial_chips,
            "cash bot chips must be conserved across table and bot bank accounts: initial={initial_chips}, table={table_chips}, bank={bank_chips}"
        );
    }

    #[tokio::test]
    async fn bot_tournament_pays_prize_pool_without_cashing_out_stacks() {
        let root = std::env::temp_dir().join(format!("two-seven-tournament-{}", Uuid::new_v4()));
        let bank = BankStore::load(&root).await.unwrap();
        let blitz = BlitzStore::load(&root).await.unwrap();
        let tables = TableStore::load(&root).await.unwrap();
        let users = Arc::new(UserStore::load(&root).await.unwrap());
        let history = crate::history::HistoryStore::load(&root).await.unwrap();
        let stats = crate::stats::StatsStore::load(&root).await.unwrap();
        let state = AppState {
            users,
            bank: bank.clone(),
            blackjack: crate::blackjack::BlackjackStore::new(),
            blitz,
            tables: tables.clone(),
            history,
            stats,
            admin_password: Arc::new("test-admin-password".into()),
            webauthn: Arc::new(build_webauthn().unwrap()),
            key: Key::generate(),
            passkey_disabled: true,
        };
        let kinds = [
            BotKind::Fish,
            BotKind::Rock,
            BotKind::Grinder,
            BotKind::Shark,
        ];
        let config = TournamentConfig {
            buy_in: 100,
            seat_count: 4,
            starting_chips: 100,
            levels: vec![
                BlindLevel {
                    small_blind: 1,
                    big_blind: 2,
                    ante: 1,
                    hands: 1,
                },
                BlindLevel {
                    small_blind: 10,
                    big_blind: 20,
                    ante: 2,
                    hands: 2,
                },
            ],
            payout_percentages: vec![65, 35],
            no_debt: false,
        };
        let mut table = Table::new(
            "tournament".into(),
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            TableMode::Tournament(TournamentState {
                config: config.clone(),
                current_level: 0,
                hands_at_level: 0,
                finish_order: Vec::new(),
                registered: 4,
                started: true,
                prize_pool: 400,
                finished: false,
                paid_out: false,
            }),
            4,
            100,
        );
        for (seat, kind) in kinds.into_iter().enumerate() {
            bank.buy_in(
                AccountOwner::Bot(crate::table::Bot::new(kind, 0)),
                table.id,
                100,
                false,
            )
            .await
            .unwrap();
            table.seats[seat] = Seat {
                occupant: SeatOccupant::bot(crate::table::Bot::new(kind, 0)),
                stack: 100,
                sitting_out: false,
                pending_departure: false,
            };
        }
        let id = tables.insert(table).await.unwrap();
        let mut now = Utc::now();
        for _ in 0..20_000 {
            now += Duration::seconds(1);
            keep_dealing(&tables, id).await;
            tick_once_at(&state, now).await.unwrap();
            let table = tables.get(id).await.unwrap();
            if let TableMode::Tournament(tournament) = &table.lock().await.mode
                && tournament.finished
                && tournament.paid_out
            {
                break;
            }
        }
        let table = tables.get(id).await.unwrap();
        let table = table.lock().await;
        let TableMode::Tournament(tournament) = &table.mode else {
            panic!("expected tournament");
        };
        assert!(tournament.finished);
        assert!(tournament.paid_out);
        assert_eq!(tournament.finish_order.len(), 3);
        assert!(tournament.current_level > 0);
        let winner = table
            .seats
            .iter()
            .enumerate()
            .find_map(|(seat, value)| (value.stack > 0).then_some(seat))
            .expect("one tournament winner");
        drop(table);
        let mut prizes = 0;
        for (seat, kind) in kinds.into_iter().enumerate() {
            let account = bank
                .account(AccountOwner::Bot(crate::table::Bot::new(kind, 0)))
                .await
                .unwrap();
            let entries = account
                .entries
                .iter()
                .filter_map(|entry| match entry.kind {
                    LedgerKind::TournamentPrize { .. } => Some(entry.delta),
                    _ => None,
                })
                .collect::<Vec<_>>();
            prizes += entries.iter().sum::<i64>();
            if seat == winner {
                assert!(account.entries.iter().any(|entry| {
                    matches!(entry.kind, LedgerKind::TournamentPrize { .. })
                        && entry.memo.contains("place 1")
                        && entry.delta == 260
                }));
            }
        }
        assert_eq!(prizes, 400);
    }

    #[tokio::test]
    async fn pending_departure_forfeits_tournament_chips_but_cash_departure_cashouts() {
        let root = std::env::temp_dir().join(format!("two-seven-departure-{}", Uuid::new_v4()));
        let bank = BankStore::load(&root).await.unwrap();
        let blitz = BlitzStore::load(&root).await.unwrap();
        let tables = TableStore::load(&root).await.unwrap();
        let users = Arc::new(UserStore::load(&root).await.unwrap());
        let history = crate::history::HistoryStore::load(&root).await.unwrap();
        let stats = crate::stats::StatsStore::load(&root).await.unwrap();
        let state = AppState {
            users,
            bank: bank.clone(),
            blackjack: crate::blackjack::BlackjackStore::new(),
            blitz,
            tables: tables.clone(),
            history,
            stats,
            admin_password: Arc::new("test-admin-password".into()),
            webauthn: Arc::new(build_webauthn().unwrap()),
            key: Key::generate(),
            passkey_disabled: true,
        };
        let kind = BotKind::Fish;
        let mut tournament = Table::new(
            "departure tournament".into(),
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            TableMode::Tournament(TournamentState {
                config: TournamentConfig {
                    buy_in: 100,
                    seat_count: 2,
                    starting_chips: 100,
                    levels: vec![BlindLevel {
                        small_blind: 1,
                        big_blind: 2,
                        ante: 0,
                        hands: 10,
                    }],
                    payout_percentages: vec![100],
                    no_debt: false,
                },
                current_level: 0,
                hands_at_level: 0,
                finish_order: Vec::new(),
                registered: 2,
                started: true,
                prize_pool: 200,
                finished: false,
                paid_out: false,
            }),
            2,
            100,
        );
        bank.buy_in(
            AccountOwner::Bot(crate::table::Bot::new(kind, 0)),
            tournament.id,
            100,
            false,
        )
        .await
        .unwrap();
        tournament.seats[0] = Seat {
            occupant: SeatOccupant::bot(crate::table::Bot::new(kind, 0)),
            stack: 50,
            sitting_out: true,
            pending_departure: true,
        };
        let tournament_id = tables.insert(tournament).await.unwrap();
        settle_pending_departures(&state, tournament_id)
            .await
            .unwrap();
        let account = bank
            .account(AccountOwner::Bot(crate::table::Bot::new(kind, 0)))
            .await
            .unwrap();
        assert_eq!(
            account
                .entries
                .iter()
                .filter(|entry| matches!(entry.kind, LedgerKind::CashOut { .. }))
                .count(),
            0
        );

        let mut cash = Table::new(
            "departure cash".into(),
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            TableMode::Cash { no_debt: false },
            2,
            100,
        );
        bank.buy_in(
            AccountOwner::Bot(crate::table::Bot::new(kind, 0)),
            cash.id,
            100,
            false,
        )
        .await
        .unwrap();
        cash.seats[0] = Seat {
            occupant: SeatOccupant::bot(crate::table::Bot::new(kind, 0)),
            stack: 50,
            sitting_out: true,
            pending_departure: true,
        };
        let cash_id = tables.insert(cash).await.unwrap();
        settle_pending_departures(&state, cash_id).await.unwrap();
        let account = bank
            .account(AccountOwner::Bot(crate::table::Bot::new(kind, 0)))
            .await
            .unwrap();
        assert_eq!(
            account
                .entries
                .iter()
                .filter(|entry| matches!(entry.kind, LedgerKind::CashOut { .. }))
                .map(|entry| entry.delta)
                .sum::<i64>(),
            50
        );
    }
}
