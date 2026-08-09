use crate::{
    app::AppState,
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
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{AppState, build_webauthn},
        bank::{AccountOwner, BankStore},
        store::TableStore,
        table::{BotKind, Seat, Stakes, Table, TableMode},
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
}
