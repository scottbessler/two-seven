use crate::{
    app::AppState,
    bank::{AccountOwner, LedgerKind},
    table::{
        SeatOccupant, Table, TableMode, maybe_start_hand, run_turn_clock, settle_finished_hand,
        turn_clock_due,
    },
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
    state
        .blackjack
        .tick(now, &state.bank, &state.blackjack_stats)
        .await;
    let mut ids = state.tables.ids().await;
    ids.sort();
    for id in ids {
        let Some(table) = state.tables.get(id).await else {
            continue;
        };
        let should_update = {
            let table = table.lock().await;
            driver_update_due(&table, now)
        };
        let mut recorded = Vec::new();
        if should_update
            && let Err(error) = state
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
                    // Whoever is to act, the clock is the first thing due: it
                    // is wound for a person, taken away from anybody who is not
                    // one, and, once it has run out, spent on their behalf.
                    if run_turn_clock(table, now).is_some() {
                        if table.hand.as_ref().is_some_and(|hand| hand.complete) {
                            recorded.extend(settle_finished_hand(table));
                        }
                        return Ok(());
                    }
                    // A parked runout has its own clock, always armed so a
                    // table nobody is watching still finishes the board. A
                    // press only ever beats this deadline to it (§V59).
                    if table
                        .hand
                        .as_ref()
                        .is_some_and(crate::holdem::Hand::awaits_runout)
                    {
                        if table.next_action_at.is_none() {
                            table.next_action_at =
                                Some(now + Duration::seconds(crate::table::RUNOUT_STEP_SECONDS));
                            return Ok(());
                        }
                        if table.next_action_at.is_some_and(|at| at > now) {
                            return Ok(());
                        }
                        if let Some(hand) = table.hand.as_mut() {
                            hand.advance_runout();
                        }
                        table.next_action_at = None;
                        if table.hand.as_ref().is_some_and(|hand| hand.complete) {
                            recorded.extend(settle_finished_hand(table));
                        }
                        return Ok(());
                    }
                    let Some(seat) = table.hand.as_ref().and_then(|hand| hand.current_player)
                    else {
                        return Ok(());
                    };
                    let Some(bot) = table
                        .seats
                        .get(seat)
                        .and_then(|value| value.occupant.as_bot())
                    else {
                        return Ok(());
                    };
                    let Some(hand) = table.hand.as_mut() else {
                        return Ok(());
                    };
                    if table.next_action_at.is_none() {
                        table.next_action_at = Some(now + Duration::milliseconds(400));
                        return Ok(());
                    }
                    if table.next_action_at.is_some_and(|at| at > now) {
                        return Ok(());
                    }
                    let view = hand_view(hand, Some(seat), &[]);
                    let legal = view
                        .legal_actions
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("bot turn has no legal actions"))?;
                    let action = bot.act(
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
                    // The house has moved; if a person is next, their clock
                    // starts with the same update the table sees.
                    run_turn_clock(table, now);
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
        if let Err(error) = seat_pending_arrivals(state, id).await {
            tracing::warn!(%id, %error, "seating a waiting player failed");
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

fn driver_update_due(table: &Table, now: DateTime<Utc>) -> bool {
    if table.hand.as_ref().is_some_and(|hand| hand.complete) {
        return true;
    }
    let Some(hand) = &table.hand else {
        return table.next_action_at.is_none_or(|at| at <= now) && can_start_hand(table);
    };
    // A person to act is the driver's business too: somebody has to hold them
    // to their ten seconds.
    if turn_clock_due(table, now) {
        return true;
    }
    // A parked runout is due on its own deadline, bot to act or not.
    if hand.awaits_runout() {
        return table.next_action_at.is_none_or(|at| at <= now);
    }
    let Some(seat) = hand.current_player else {
        return false;
    };
    if table
        .seats
        .get(seat)
        .and_then(|value| value.occupant.as_bot())
        .is_none()
    {
        return false;
    }
    table.next_action_at.is_none_or(|at| at <= now)
}

fn can_start_hand(table: &Table) -> bool {
    if let TableMode::Tournament(state) = &table.mode
        && !state.started
        && state.registered < state.config.seat_count
    {
        return false;
    }
    if table.hand.is_some() || deal_in_count(table) < 2 {
        return false;
    }
    if table.cash_tier.is_some()
        && table
            .seats
            .iter()
            .any(|seat| matches!(seat.occupant, SeatOccupant::Empty))
    {
        return false;
    }
    !table.waits_for_a_watcher() || table.bot_hands_requested > 0
}

fn deal_in_count(table: &Table) -> usize {
    table
        .seats
        .iter()
        .filter(|seat| {
            !seat.sitting_out && seat.stack > 0 && !matches!(seat.occupant, SeatOccupant::Empty)
        })
        .count()
}

/// Cash tables built by hand before the ladder existed have nowhere to belong,
/// so they are paid out and retired at startup. Tournaments are still made by
/// players and are left alone.
pub async fn retire_custom_cash_tables(state: &AppState) -> Result<(), anyhow::Error> {
    for id in state.tables.ids().await {
        let Some(table) = state.tables.get(id).await else {
            continue;
        };
        let (stacks, waiting, buy_in) = {
            let table = table.lock().await;
            if table.cash_tier.is_some()
                || !matches!(table.mode, crate::table::TableMode::Cash { .. })
            {
                continue;
            }
            (
                table
                    .seats
                    .iter()
                    .filter(|seat| seat.stack > 0)
                    .map(|seat| (seat.occupant.clone(), seat.stack))
                    .collect::<Vec<_>>(),
                table
                    .seats
                    .iter()
                    .filter_map(|seat| seat.pending_arrival)
                    .collect::<Vec<_>>(),
                table.buy_in,
            )
        };
        // A seat somebody paid for but never got is given back to them.
        for user in waiting {
            if let Err(error) = state
                .bank
                .cash_out(AccountOwner::User(user), id, buy_in)
                .await
            {
                tracing::warn!(%id, %error, "refunding a waiting player failed");
            }
        }
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
/// How long an unregistered tournament is left standing before it is swept.
/// Long enough that one just created, and being looked at, is never touched.
pub const ABANDONED_TOURNAMENT_HOURS: i64 = 24;

/// Forget tournaments nobody ever entered.
///
/// Creating a tournament is one press, and the ones nobody registers for never
/// start, never finish and never get cleared, so they pile up: the lobby lists
/// every one of them and the driver walks every one of them on every tick. A
/// table with a registered player is somebody's money and is left alone —
/// only an empty one past its grace period is swept, so nothing is refunded
/// here and nothing can be lost (§V64).
pub async fn retire_abandoned_tournaments(state: &AppState) -> Result<(), anyhow::Error> {
    let cutoff = Utc::now() - Duration::hours(ABANDONED_TOURNAMENT_HOURS);
    let mut swept = 0;
    for id in state.tables.ids().await {
        let Some(table) = state.tables.get(id).await else {
            continue;
        };
        let abandoned = {
            let table = table.lock().await;
            match &table.mode {
                TableMode::Tournament(tournament) => {
                    !tournament.started
                        && !tournament.finished
                        && tournament.registered == 0
                        && tournament.prize_pool == 0
                        && table.updated_at < cutoff
                }
                TableMode::Cash { .. } => false,
            }
        };
        if abandoned {
            state.tables.remove(id).await?;
            swept += 1;
        }
    }
    if swept > 0 {
        tracing::info!(swept, "retired tournaments nobody entered");
    }
    Ok(())
}

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
    reseat_house_players_off_the_mix(state).await;
    Ok(())
}

/// Stand up any house player the table's own lineup does not call for (§V62),
/// so a saved table converges on the mix its rung is supposed to have. Two
/// things put a table off it: a kind the rung no longer allows at all, and a
/// kind seated more often than the mix asks for -- a table saved while the
/// thresholds sat a band lower can be all sharks at a rung that wants a
/// spread. Either way the seat empties and the tick refills it from
/// `house_bot`, which seats the kind that seat calls for.
async fn reseat_house_players_off_the_mix(state: &AppState) {
    for id in state.tables.ids().await {
        let Some(table) = state.tables.get(id).await else {
            continue;
        };
        let off_the_mix = {
            let table = table.lock().await;
            let Some(tier) = table.cash_tier else {
                continue;
            };
            if table.hand.is_some() {
                continue;
            }
            // A seat keeps its house player when they are the kind that seat
            // calls for, and stands them up otherwise. Judging it seat by seat
            // is what makes one pass enough: `house_bot` fills a vacancy from
            // the same per-seat order, so the table lands on the mix instead
            // of trading one surplus for another.
            let wanted = crate::cash::seating_order(tier);
            let off_the_mix: Vec<(usize, crate::table::Bot, crate::money::Cents)> = table
                .seats
                .iter()
                .enumerate()
                .filter_map(|(index, seat)| {
                    let bot = seat.occupant.as_bot()?;
                    let calls_for = wanted.get(index % wanted.len())?;
                    (bot.kind != *calls_for).then_some((index, bot, seat.stack))
                })
                .collect();
            off_the_mix
        };
        for (index, bot, stack) in off_the_mix {
            if state
                .bank
                .cash_out(AccountOwner::Bot(bot), id, stack)
                .await
                .is_err()
            {
                continue;
            }
            if let Err(error) = state
                .tables
                .update(id, |table| {
                    if let Some(seat) = table.seats.get_mut(index)
                        && seat.occupant.as_bot() == Some(bot)
                    {
                        seat.occupant = SeatOccupant::Empty;
                        seat.stack = 0;
                        seat.sitting_out = false;
                        seat.pending_departure = false;
                    }
                    Ok(())
                })
                .await
            {
                tracing::warn!(%id, %error, "could not stand up a house player off the mix");
            }
        }
    }
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
        let Some(vacancy) = table.seats.iter().position(|seat| {
            matches!(seat.occupant, SeatOccupant::Empty) && seat.pending_arrival.is_none()
        }) else {
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

/// Hands over the seats people paid for while the house was still mid-hand.
/// The swap waits for the hand to end so that nobody is dealt into a hand they
/// did not pay into, and so that settlement writes the house player's chips to
/// the house player rather than over the newcomer's buy-in.
async fn seat_pending_arrivals(state: &AppState, id: uuid::Uuid) -> Result<(), anyhow::Error> {
    let table = state
        .tables
        .get(id)
        .await
        .ok_or_else(|| anyhow::anyhow!("table missing"))?;
    let (buy_in, arrivals) = {
        let table = table.lock().await;
        // Never rearrange a table mid-hand.
        if table.hand.is_some() {
            return Ok(());
        }
        (
            table.buy_in,
            table
                .seats
                .iter()
                .enumerate()
                .filter_map(|(seat, value)| value.pending_arrival.map(|user| (seat, user)))
                .collect::<Vec<_>>(),
        )
    };
    for (seat, user) in arrivals {
        let mut displaced = None;
        state
            .tables
            .update(id, |table| {
                if table.hand.is_some() {
                    return Ok(());
                }
                let value = table
                    .seats
                    .get_mut(seat)
                    .ok_or_else(|| anyhow::anyhow!("seat missing"))?;
                if value.pending_arrival != Some(user) {
                    return Ok(());
                }
                displaced = value.occupant.as_bot().map(|bot| (bot, value.stack));
                *value = crate::table::Seat {
                    occupant: SeatOccupant::Human { user_id: user },
                    stack: buy_in,
                    sitting_out: false,
                    pending_departure: false,
                    pending_arrival: None,
                };
                Ok(())
            })
            .await?;
        // A house player who lost their seat takes their settled chips with it.
        if let Some((bot, stack)) = displaced {
            state
                .bank
                .cash_out(AccountOwner::Bot(bot), id, stack)
                .await?;
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

    /// An unstarted tournament nobody entered is swept once it is old enough;
    /// one somebody has paid into, and one only just made, are left standing
    /// (§V64).
    #[tokio::test]
    async fn only_stale_tournaments_nobody_entered_are_swept() {
        let root = std::env::temp_dir().join(format!("two-seven-sweep-{}", Uuid::new_v4()));
        let tables = TableStore::load(&root).await.unwrap();
        let state = AppState {
            users: Arc::new(UserStore::load(&root).await.unwrap()),
            bank: BankStore::load(&root).await.unwrap(),
            blackjack: crate::blackjack::BlackjackStore::new(),
            blackjack_stats: crate::blackjack_stats::BlackjackStatsStore::new(),
            blitz: BlitzStore::load(&root).await.unwrap(),
            tables: tables.clone(),
            history: crate::history::HistoryStore::load(&root).await.unwrap(),
            stats: crate::stats::StatsStore::load(&root).await.unwrap(),
            admin_password: Arc::new("test-admin-password".into()),
            webauthn: Arc::new(build_webauthn().unwrap()),
            key: Key::generate(),
            passkey_disabled: true,
        };

        let empty_tournament = |registered: usize, prize_pool: crate::money::Cents| {
            Table::new(
                "tournament".into(),
                Stakes::NoLimit {
                    small_blind: 1,
                    big_blind: 2,
                },
                TableMode::Tournament(TournamentState {
                    config: TournamentConfig {
                        buy_in: 100,
                        seat_count: 6,
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
                    registered,
                    started: false,
                    prize_pool,
                    finished: false,
                    paid_out: false,
                }),
                6,
                100,
            )
        };
        let stale = tables.insert(empty_tournament(0, 0)).await.unwrap();
        let entered = tables.insert(empty_tournament(1, 100)).await.unwrap();
        let fresh = tables.insert(empty_tournament(0, 0)).await.unwrap();
        // Age the two that are meant to be old enough to sweep; `fresh` keeps
        // the timestamp `insert` just gave it. `TableStore::update` stamps
        // `updated_at` with the current time itself, so this reaches past it.
        let long_ago = Utc::now() - Duration::hours(ABANDONED_TOURNAMENT_HOURS + 1);
        for id in [stale, entered] {
            tables.get(id).await.unwrap().lock().await.updated_at = long_ago;
        }

        retire_abandoned_tournaments(&state).await.unwrap();

        assert!(
            tables.get(stale).await.is_none(),
            "an old tournament nobody entered is forgotten"
        );
        assert!(
            tables.get(entered).await.is_some(),
            "somebody's money is still in this one"
        );
        assert!(
            tables.get(fresh).await.is_some(),
            "one just created is still being looked at"
        );
    }

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
            blackjack_stats: crate::blackjack_stats::BlackjackStatsStore::new(),
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
            pending_arrival: None,
        };
        table.seats[1] = crate::table::Seat {
            occupant: SeatOccupant::bot(crate::table::Bot::new(BotKind::Rock, 0)),
            stack: 19_000,
            sitting_out: false,
            pending_departure: false,
            pending_arrival: None,
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
    async fn idle_human_table_ticks_without_broadcasting() {
        let root = std::env::temp_dir().join(format!("two-seven-idle-tick-{}", Uuid::new_v4()));
        let bank = BankStore::load(&root).await.unwrap();
        let blitz = BlitzStore::load(&root).await.unwrap();
        let tables = TableStore::load(&root).await.unwrap();
        let users = Arc::new(UserStore::load(&root).await.unwrap());
        let state = AppState {
            users,
            bank,
            blackjack: crate::blackjack::BlackjackStore::new(),
            blackjack_stats: crate::blackjack_stats::BlackjackStatsStore::new(),
            blitz,
            tables: tables.clone(),
            history: crate::history::HistoryStore::load(&root).await.unwrap(),
            stats: crate::stats::StatsStore::load(&root).await.unwrap(),
            admin_password: Arc::new("test-admin-password".into()),
            webauthn: Arc::new(build_webauthn().unwrap()),
            key: Key::generate(),
            passkey_disabled: true,
        };
        let mut table = Table::new(
            "idle".into(),
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            TableMode::Cash { no_debt: false },
            2,
            20_000,
        );
        table.seats[0] = Seat {
            occupant: SeatOccupant::Human {
                user_id: Uuid::new_v4(),
            },
            stack: 20_000,
            sitting_out: false,
            pending_departure: false,
            pending_arrival: None,
        };
        let id = tables.insert(table).await.unwrap();
        let mut events = tables.subscribe();

        tick_once_at(&state, Utc::now()).await.unwrap();

        let event = tokio::time::timeout(std::time::Duration::from_millis(50), events.recv()).await;
        assert!(
            event.is_err(),
            "idle driver ticks should not broadcast table {id}"
        );
    }

    #[tokio::test]
    async fn the_driver_acts_for_a_person_whose_time_runs_out() {
        let root = std::env::temp_dir().join(format!("two-seven-turn-clock-{}", Uuid::new_v4()));
        let tables = TableStore::load(&root).await.unwrap();
        let state = AppState {
            users: Arc::new(UserStore::load(&root).await.unwrap()),
            bank: BankStore::load(&root).await.unwrap(),
            blackjack: crate::blackjack::BlackjackStore::new(),
            blackjack_stats: crate::blackjack_stats::BlackjackStatsStore::new(),
            blitz: BlitzStore::load(&root).await.unwrap(),
            tables: tables.clone(),
            history: crate::history::HistoryStore::load(&root).await.unwrap(),
            stats: crate::stats::StatsStore::load(&root).await.unwrap(),
            admin_password: Arc::new("test-admin-password".into()),
            webauthn: Arc::new(build_webauthn().unwrap()),
            key: Key::generate(),
            passkey_disabled: true,
        };
        let mut table = Table::new(
            "two of us".into(),
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            TableMode::Cash { no_debt: false },
            2,
            20_000,
        );
        for seat in table.seats.iter_mut() {
            seat.occupant = SeatOccupant::Human {
                user_id: Uuid::new_v4(),
            };
            seat.stack = 20_000;
        }
        let id = tables.insert(table).await.unwrap();

        let start = Utc::now();
        // The first tick deals; the second puts the player to act on the clock.
        tick_once_at(&state, start).await.unwrap();
        tick_once_at(&state, start).await.unwrap();
        let seat = {
            let table = tables.get(id).await.unwrap();
            let table = table.lock().await;
            let hand = table.hand.as_ref().expect("a hand is dealt");
            assert_eq!(
                table.turn_clock.map(|clock| clock.deadline),
                Some(start + Duration::seconds(crate::table::TURN_SECONDS)),
                "the person to act is on the clock",
            );
            hand.current_player.expect("somebody is to act")
        };

        // Nothing happens while they still have time.
        tick_once_at(&state, start + Duration::seconds(9))
            .await
            .unwrap();
        {
            let table = tables.get(id).await.unwrap();
            let table = table.lock().await;
            assert_eq!(table.hand.as_ref().unwrap().current_player, Some(seat));
        }

        // Once it runs out the table folds for them, which heads-up ends the
        // hand -- and the hand is settled like any other.
        tick_once_at(&state, start + Duration::seconds(11))
            .await
            .unwrap();
        let table = tables.get(id).await.unwrap();
        let table = table.lock().await;
        assert!(table.hand.is_none(), "the folded hand is settled");
        assert_eq!(table.turn_clock, None);
        let summary = table.last_hand.as_ref().expect("a result to show");
        assert!(
            summary.awards.iter().all(|award| award.seat != seat),
            "the seat that timed out wins nothing",
        );
        assert!(
            state.history.recent(id, 10).await.len() == 1,
            "the hand it ended is written to history",
        );
    }

    #[tokio::test]
    async fn blackjack_driver_stands_the_current_hand_after_its_deadline() {
        let root =
            std::env::temp_dir().join(format!("two-seven-blackjack-clock-{}", Uuid::new_v4()));
        let bank = BankStore::load(&root).await.unwrap();
        let blackjack_stats = crate::blackjack_stats::BlackjackStatsStore::new();
        let users = Arc::new(UserStore::load(&root).await.unwrap());
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();
        let now = Utc::now();
        let mut table = crate::blackjack::BlackjackTable::new(0);
        for (index, user) in [user_a, user_b].into_iter().enumerate() {
            table.seats[index] = Some(crate::blackjack::BlackjackSeat {
                user,
                stack: 7_500,
                bet: Some(2_500),
                hands: vec![crate::blackjack::BlackjackHand {
                    cards: vec![
                        crate::cards::Card::new(
                            crate::cards::Rank::Eight,
                            crate::cards::Suit::Spades,
                        ),
                        crate::cards::Card::new(
                            crate::cards::Rank::Seven,
                            crate::cards::Suit::Hearts,
                        ),
                    ],
                    bet: 2_500,
                    status: crate::blackjack::BlackjackHandStatus::Playing,
                    split: false,
                    split_aces: false,
                    doubled: false,
                }],
                insurance: 0,
                insurance_decided: false,
                leaving: false,
                settings: Default::default(),
                decisions: Vec::new(),
            });
        }
        table.phase = crate::blackjack::Phase::Playing;
        table.current = Some((0, 0));
        table.deadline = Some(now + Duration::seconds(10));
        let blackjack = crate::blackjack::BlackjackStore::from_tables(vec![table]);
        let state = AppState {
            users,
            bank: bank.clone(),
            blackjack: blackjack.clone(),
            blackjack_stats: blackjack_stats.clone(),
            blitz: BlitzStore::load(&root).await.unwrap(),
            tables: TableStore::load(&root).await.unwrap(),
            history: crate::history::HistoryStore::load(&root).await.unwrap(),
            stats: crate::stats::StatsStore::load(&root).await.unwrap(),
            admin_password: Arc::new("test-admin-password".into()),
            webauthn: Arc::new(build_webauthn().unwrap()),
            key: Key::generate(),
            passkey_disabled: true,
        };
        let before = blackjack
            .view(crate::blackjack::table_id(0), Some(user_a), 0)
            .await
            .unwrap();
        let (seat, hand) = (
            before.current_seat.expect("current seat"),
            before.current_hand.expect("current hand"),
        );
        let deadline = before.deadline.expect("playing deadline");
        tick_once_at(&state, deadline + Duration::seconds(1))
            .await
            .unwrap();
        let after = blackjack
            .view(crate::blackjack::table_id(0), Some(user_a), 0)
            .await
            .unwrap();
        assert_eq!(
            after.seats[seat].hands[hand].status,
            crate::blackjack::BlackjackHandStatus::Stand
        );
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
            blackjack_stats: crate::blackjack_stats::BlackjackStatsStore::new(),
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
        // Nobody sits at a table their kind is not seated at (§V62).
        for id in tables.ids().await {
            let table = tables.get(id).await.unwrap();
            let table = table.lock().await;
            for bot in table.seats.iter().filter_map(|seat| seat.occupant.as_bot()) {
                assert!(
                    crate::cash::kind_allowed(table.buy_in, bot.kind),
                    "{} is out of their depth at {}",
                    bot.name(),
                    table.name
                );
            }
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

    /// V62: a table saved before the stakes constraint arrived can be holding
    /// somebody it would not seat today -- a kind the rung disallows outright,
    /// or a kind seated more often than its mix calls for. Startup stands both
    /// up and the tick refills the seats from the lineup the rung wants.
    #[tokio::test]
    async fn startup_reseats_house_players_who_are_off_the_mix() {
        let root = std::env::temp_dir().join(format!("two-seven-depth-{}", Uuid::new_v4()));
        let bank = BankStore::load(&root).await.unwrap();
        let blitz = BlitzStore::load(&root).await.unwrap();
        let tables = TableStore::load(&root).await.unwrap();
        let users = Arc::new(UserStore::load(&root).await.unwrap());
        let history = crate::history::HistoryStore::load(&root).await.unwrap();
        let stats = crate::stats::StatsStore::load(&root).await.unwrap();
        let blackjack_stats = crate::blackjack_stats::BlackjackStatsStore::load(&root)
            .await
            .unwrap();
        let state = AppState {
            users,
            bank,
            blackjack: crate::blackjack::BlackjackStore::new(),
            blackjack_stats,
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
        // The dearest rung takes sharks only; put a fish there as an old save
        // would have.
        let dearest = {
            let mut found = None;
            for id in tables.ids().await {
                let table = tables.get(id).await.unwrap();
                let tier = table.lock().await.cash_tier;
                if tier == Some(crate::cash::TIERS.len() - 1) {
                    found = Some(id);
                }
            }
            found.expect("the dearest table")
        };
        let fish = crate::table::Bot::new(BotKind::Fish, 0);
        tables
            .update(dearest, |table| {
                table.seats[0].occupant = SeatOccupant::bot(fish);
                table.seats[0].stack = table.buy_in;
                Ok(())
            })
            .await
            .unwrap();
        // A rung that wants a spread can be saved full of sharks -- every one
        // of them allowed there, so an allow-list check alone would leave it
        // alone. This is what a table saved while the thresholds sat a band
        // lower looks like.
        let (middling, tier) = {
            let mut found = None;
            for id in tables.ids().await {
                let table = tables.get(id).await.unwrap();
                let tier = table.lock().await.cash_tier;
                if tier == Some(4) {
                    found = Some((id, 4));
                }
            }
            found.expect("the $10,000 table")
        };
        let wanted = crate::cash::seating_order(tier);
        assert!(
            wanted.iter().any(|kind| *kind != BotKind::Shark),
            "this rung is supposed to want a spread: {wanted:?}"
        );
        tables
            .update(middling, |table| {
                for (index, seat) in table.seats.iter_mut().enumerate() {
                    seat.occupant =
                        SeatOccupant::bot(crate::table::Bot::new(BotKind::Shark, index as u8));
                    seat.stack = table.buy_in;
                }
                Ok(())
            })
            .await
            .unwrap();

        ensure_cash_ladder(&state).await.unwrap();
        let table = tables.get(dearest).await.unwrap();
        let table = table.lock().await;
        assert!(
            !table
                .seats
                .iter()
                .any(|seat| seat.occupant.as_bot() == Some(fish)),
            "a fish does not keep a seat at the dearest table"
        );
        assert_eq!(table.seats[0].stack, 0, "the seat is cleared, not stranded");
        drop(table);

        // The surplus sharks stood up; the tick fills what they left with the
        // kinds the rung actually calls for.
        let seated_sharks = |table: &crate::table::Table| {
            table
                .seats
                .iter()
                .filter(|seat| seat.occupant.as_bot().map(|bot| bot.kind) == Some(BotKind::Shark))
                .count()
        };
        let sharks_wanted = wanted
            .iter()
            .filter(|kind| **kind == BotKind::Shark)
            .count();
        {
            let table = tables.get(middling).await.unwrap();
            let table = table.lock().await;
            assert_eq!(
                seated_sharks(&table),
                sharks_wanted,
                "the rung keeps only the sharks its mix asks for"
            );
        }
        for _ in 0..(crate::cash::SEATS * crate::cash::TIERS.len() + 4) {
            tick_once(&state).await.unwrap();
        }
        let table = tables.get(middling).await.unwrap();
        let table = table.lock().await;
        let seated: Vec<_> = table
            .seats
            .iter()
            .filter_map(|seat| seat.occupant.as_bot().map(|bot| bot.kind))
            .collect();
        assert_eq!(seated.len(), crate::cash::SEATS, "the table refills");
        assert!(
            seated.iter().any(|kind| *kind != BotKind::Shark),
            "the rung is no longer all sharks: {seated:?}"
        );
        assert_eq!(
            seated_sharks(&table),
            sharks_wanted,
            "the refilled lineup matches the mix"
        );
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
            blackjack_stats: crate::blackjack_stats::BlackjackStatsStore::new(),
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
            pending_arrival: None,
        };
        table.seats[1] = Seat {
            occupant: SeatOccupant::bot(crate::table::Bot::new(BotKind::Fish, 0)),
            stack: 100,
            sitting_out: false,
            pending_departure: false,
            pending_arrival: None,
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
            blackjack_stats: crate::blackjack_stats::BlackjackStatsStore::new(),
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
            blackjack_stats: crate::blackjack_stats::BlackjackStatsStore::new(),
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
                pending_arrival: None,
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
            "cash bot chips must be conserved: initial={initial_chips}, table={table_chips}, bank={bank_chips}"
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
            blackjack_stats: crate::blackjack_stats::BlackjackStatsStore::new(),
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
                pending_arrival: None,
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
            let account = bank
                .account(AccountOwner::Bot(crate::table::Bot::new(kind, 0)))
                .await
                .unwrap();
            bank_chips += account.balance;
        }
        assert_eq!(
            table_chips + bank_chips,
            initial_chips,
            "cash bot chips must be conserved: initial={initial_chips}, table={table_chips}, bank={bank_chips}"
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
            blackjack_stats: crate::blackjack_stats::BlackjackStatsStore::new(),
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
                pending_arrival: None,
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
            blackjack_stats: crate::blackjack_stats::BlackjackStatsStore::new(),
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
            pending_arrival: None,
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
            pending_arrival: None,
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
