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
    for id in state.tables.ids().await {
        if state.tables.get(id).await.is_none() {
            continue;
        }
        state
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
            .await?;
        pay_tournament_if_finished(state, id).await?;
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
        let mut order = tournament.finish_order.clone();
        for seat in (0..table.seats.len()).rev() {
            if table.seats[seat].stack > 0
                && !matches!(table.seats[seat].occupant, SeatOccupant::Empty)
                && !order.contains(&seat)
            {
                order.insert(0, seat);
            }
        }
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
    for (position, owner) in owners {
        let Some(percent) = payouts.get(position) else {
            break;
        };
        let amount = if position + 1 == payouts.len() {
            prize_pool - paid
        } else {
            prize_pool * i64::from(*percent) / 100
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
    async fn four_bots_complete_hands_without_losing_chips() {
        let root = std::env::temp_dir().join(format!("two-seven-driver-{}", Uuid::new_v4()));
        let bank = BankStore::load(&root).await.unwrap();
        let tables = TableStore::load(&root).await.unwrap();
        let users = Arc::new(UserStore::load(&root).await.unwrap());
        let state = AppState {
            users,
            bank: bank.clone(),
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
            10,
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
            };
        }
        let id = tables.insert(table).await.unwrap();
        let mut now = Utc::now();
        for _ in 0..2_000 {
            now += Duration::seconds(1);
            tick_once_at(&state, now).await.unwrap();
        }
        let table = tables.get(id).await.unwrap();
        let table = table.lock().await;
        assert!(table.hand_no > 5);
        assert_eq!(table.seats.iter().map(|seat| seat.stack).sum::<i64>(), 400);
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
        let tables = TableStore::load(&root).await.unwrap();
        let users = Arc::new(UserStore::load(&root).await.unwrap());
        let state = AppState {
            users,
            bank: bank.clone(),
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
            10,
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
        assert_eq!(table.seats.iter().map(|seat| seat.stack).sum::<i64>(), 400);
    }

    #[tokio::test]
    async fn bot_tournament_pays_prize_pool_without_cashing_out_stacks() {
        let root = std::env::temp_dir().join(format!("two-seven-tournament-{}", Uuid::new_v4()));
        let bank = BankStore::load(&root).await.unwrap();
        let tables = TableStore::load(&root).await.unwrap();
        let users = Arc::new(UserStore::load(&root).await.unwrap());
        let state = AppState {
            users,
            bank: bank.clone(),
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
                    small_blind: 2,
                    big_blind: 4,
                    ante: 2,
                    hands: 2,
                },
            ],
            payout_percentages: vec![100],
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
                prize_pool: 400,
                finished: false,
                paid_out: false,
            }),
            4,
            100,
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
        drop(table);
        let mut prizes = 0;
        for kind in kinds {
            let account = bank.account(AccountOwner::Bot(kind)).await.unwrap();
            prizes += account
                .entries
                .iter()
                .filter_map(|entry| match entry.kind {
                    LedgerKind::TournamentPrize { .. } => Some(entry.delta),
                    _ => None,
                })
                .sum::<i64>();
        }
        assert_eq!(prizes, 400);
    }
}
