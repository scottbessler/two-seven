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
    let mut ids = state.tables.ids().await;
    ids.sort();
    for id in ids {
        if state.tables.get(id).await.is_none() {
            continue;
        }
        if let Err(error) = state
            .tables
            .update(id, |table| {
                if table.hand.as_ref().is_some_and(|hand| hand.complete) {
                    settle_finished_hand(table);
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
                let Some(SeatOccupant::Bot { kind }) =
                    table.seats.get(seat).map(|seat| &seat.occupant)
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
                let kind = *kind;
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
                    settle_finished_hand(table);
                }
                Ok(())
            })
            .await
        {
            tracing::warn!(%id, %error, "table driver update failed");
            continue;
        }
        if let Err(error) = settle_pending_departures(state, id).await {
            tracing::warn!(%id, %error, "pending departure settlement failed");
            continue;
        }
        if let Err(error) = rebuy_busted_cash_bots(state, id).await {
            tracing::warn!(%id, %error, "cash bot rebuy failed");
            continue;
        }
        if let Err(error) = pay_tournament_if_finished(state, id).await {
            tracing::warn!(%id, %error, "tournament payout failed");
        }
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
            } else if let SeatOccupant::Bot { kind } = occupant
                && stack > 0
            {
                state
                    .bank
                    .cash_out(AccountOwner::Bot(kind), id, stack)
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
        let SeatOccupant::Bot { kind } = occupant else {
            continue;
        };
        state
            .bank
            .buy_in(AccountOwner::Bot(kind), id, amount, false)
            .await?;
        if let Err(error) = state
            .tables
            .update(id, |table| {
                let value = table
                    .seats
                    .get_mut(seat)
                    .ok_or_else(|| anyhow::anyhow!("seat missing"))?;
                if matches!(&value.occupant, SeatOccupant::Bot { kind: current } if *current == kind)
                    && value.stack == 0
                {
                    value.stack = amount;
                }
                Ok(())
            })
            .await
        {
            let _ = state
                .bank
                .cash_out(AccountOwner::Bot(kind), id, amount)
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
                    SeatOccupant::Bot { kind } => AccountOwner::Bot(kind),
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
    async fn cash_bots_rebuy_after_busting() {
        let root = std::env::temp_dir().join(format!("two-seven-rebuy-{}", Uuid::new_v4()));
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
            occupant: SeatOccupant::Bot {
                kind: BotKind::Rock,
            },
            stack: 0,
            sitting_out: false,
            pending_departure: false,
        };
        table.seats[1] = Seat {
            occupant: SeatOccupant::Bot {
                kind: BotKind::Fish,
            },
            stack: 100,
            sitting_out: false,
            pending_departure: false,
        };
        bank.buy_in(AccountOwner::Bot(BotKind::Fish), table.id, 100, false)
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
            bank.account(AccountOwner::Bot(BotKind::Rock))
                .await
                .unwrap()
                .entries
                .iter()
                .any(|entry| entry.delta == -100)
        );
    }

    #[tokio::test]
    async fn four_bots_complete_hands_without_losing_chips() {
        let root = std::env::temp_dir().join(format!("two-seven-driver-{}", Uuid::new_v4()));
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
            bank.buy_in(AccountOwner::Bot(kind), table.id, 100, false)
                .await
                .unwrap();
            table.seats[seat] = Seat {
                occupant: crate::table::SeatOccupant::Bot { kind },
                stack: 100,
                sitting_out: false,
                pending_departure: false,
            };
        }
        let id = tables.insert(table).await.unwrap();
        let mut now = Utc::now();
        let mut aggressive_actions = std::collections::HashSet::new();
        for _ in 0..2_000 {
            now += Duration::seconds(1);
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
        assert!(
            table.seats.iter().map(|seat| seat.stack).sum::<i64>() >= 400,
            "cash bot rebuys should add chips from bot bankrolls"
        );
        for kind in kinds {
            let account = bank.account(AccountOwner::Bot(kind)).await.unwrap();
            assert_eq!(
                account.entries.iter().map(|entry| entry.delta).sum::<i64>(),
                account.balance
            );
        }
    }

    #[tokio::test]
    async fn four_bots_complete_limit_hands_without_losing_chips() {
        let root = std::env::temp_dir().join(format!("two-seven-limit-driver-{}", Uuid::new_v4()));
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
            bank.buy_in(AccountOwner::Bot(kind), table.id, 100, false)
                .await
                .unwrap();
            table.seats[seat] = Seat {
                occupant: SeatOccupant::Bot { kind },
                stack: 100,
                sitting_out: false,
                pending_departure: false,
            };
        }
        let id = tables.insert(table).await.unwrap();
        let mut now = Utc::now();
        for _ in 0..4_000 {
            now += Duration::seconds(1);
            tick_once_at(&state, now).await.unwrap();
        }
        let table = tables.get(id).await.unwrap();
        let table = table.lock().await;
        assert!(table.hand_no > 3);
        assert!(
            table.seats.iter().map(|seat| seat.stack).sum::<i64>() >= 400,
            "cash bot rebuys should add chips from bot bankrolls"
        );
    }

    #[tokio::test]
    async fn bot_tournament_pays_prize_pool_without_cashing_out_stacks() {
        let root = std::env::temp_dir().join(format!("two-seven-tournament-{}", Uuid::new_v4()));
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
                    hands: 2,
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
            bank.buy_in(AccountOwner::Bot(kind), table.id, 100, false)
                .await
                .unwrap();
            table.seats[seat] = Seat {
                occupant: SeatOccupant::Bot { kind },
                stack: 100,
                sitting_out: false,
                pending_departure: false,
            };
        }
        let id = tables.insert(table).await.unwrap();
        let mut now = Utc::now();
        for _ in 0..20_000 {
            now += Duration::seconds(1);
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
            let account = bank.account(AccountOwner::Bot(kind)).await.unwrap();
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
        let state = AppState {
            users,
            bank: bank.clone(),
            blackjack: crate::blackjack::BlackjackStore::new(),
            blitz,
            tables: tables.clone(),
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
        bank.buy_in(AccountOwner::Bot(kind), tournament.id, 100, false)
            .await
            .unwrap();
        tournament.seats[0] = Seat {
            occupant: SeatOccupant::Bot { kind },
            stack: 50,
            sitting_out: true,
            pending_departure: true,
        };
        let tournament_id = tables.insert(tournament).await.unwrap();
        settle_pending_departures(&state, tournament_id)
            .await
            .unwrap();
        let account = bank.account(AccountOwner::Bot(kind)).await.unwrap();
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
        bank.buy_in(AccountOwner::Bot(kind), cash.id, 100, false)
            .await
            .unwrap();
        cash.seats[0] = Seat {
            occupant: SeatOccupant::Bot { kind },
            stack: 50,
            sitting_out: true,
            pending_departure: true,
        };
        let cash_id = tables.insert(cash).await.unwrap();
        settle_pending_departures(&state, cash_id).await.unwrap();
        let account = bank.account(AccountOwner::Bot(kind)).await.unwrap();
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
