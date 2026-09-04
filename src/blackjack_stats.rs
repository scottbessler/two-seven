//! What every player has done at the blackjack table.
//!
//! Blackjack kept no record at all until now, but the bank did: every round
//! writes a `BlackjackBet` and a `BlackjackPayout` against the same game id.
//! So the store is seeded once from the ledger — which makes the board open
//! full, covering every round ever played — and watched at the table from then
//! on, which is the only way to know a bust from a dealer win, or a split from
//! a single hand. `derived_rounds` says how much of a player's record came from
//! the ledger and so has no such detail.

use crate::{
    bank::{Account, AccountOwner, LedgerKind},
    money::Cents,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlackjackStats {
    /// Rounds dealt. A split is one round holding two hands.
    pub rounds: u64,
    /// Hands settled, so a split round counts twice.
    pub hands: u64,
    pub won: u64,
    pub lost: u64,
    pub push: u64,
    pub naturals: u64,
    pub busts: u64,
    pub doubles: u64,
    pub splits: u64,
    pub insurance_taken: u64,
    /// Everything staked, base bets and doubles and insurance together.
    pub wagered: Cents,
    /// Everything paid back, winnings and returned pushes together.
    pub returned: Cents,
    /// Of `rounds`, how many were counted from the ledger rather than watched.
    /// Those carry money and a win, loss or push, but no bust, split or
    /// double — the ledger never knew.
    pub derived_rounds: u64,
}

impl BlackjackStats {
    pub fn net(&self) -> Cents {
        self.returned - self.wagered
    }

    pub fn win_percent(&self) -> u64 {
        (self.won * 100).checked_div(self.hands).unwrap_or(0)
    }

    /// True once the store has watched at least one round for this player, so
    /// the detail columns say something.
    pub fn watched(&self) -> bool {
        self.rounds > self.derived_rounds
    }
}

/// One settled round, as the table saw it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RoundOutcome {
    pub hands: u64,
    pub won: u64,
    pub lost: u64,
    pub push: u64,
    pub naturals: u64,
    pub busts: u64,
    pub doubles: u64,
    pub splits: u64,
    pub insured: bool,
    pub wagered: Cents,
    pub returned: Cents,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct StatsFile {
    users: HashMap<Uuid, BlackjackStats>,
    /// Set once the ledger has been walked, so the seeding happens exactly
    /// once and a round is never counted twice.
    #[serde(default)]
    backfilled_at: Option<DateTime<Utc>>,
}

#[derive(Clone)]
pub struct BlackjackStatsStore {
    inner: Arc<Mutex<StatsFile>>,
    path: Option<PathBuf>,
}

impl Default for BlackjackStatsStore {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(StatsFile::default())),
            path: None,
        }
    }
}

impl BlackjackStatsStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn load(root: impl AsRef<Path>) -> Result<Self> {
        let dir = root.as_ref().join("blackjack");
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join("player-stats.json");
        let file = match tokio::fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice::<StatsFile>(&bytes).unwrap_or_default(),
            Err(_) => StatsFile::default(),
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(file)),
            path: Some(path),
        })
    }

    pub async fn all(&self) -> HashMap<Uuid, BlackjackStats> {
        self.inner.lock().await.users.clone()
    }

    pub async fn of(&self, user: Uuid) -> BlackjackStats {
        self.inner
            .lock()
            .await
            .users
            .get(&user)
            .copied()
            .unwrap_or_default()
    }

    pub async fn reset_all(&self) -> Result<usize> {
        let removed = {
            let mut guard = self.inner.lock().await;
            let removed = guard.users.len();
            // The watermark goes too: a reset should be able to seed itself
            // again from the ledger rather than come back empty.
            *guard = StatsFile::default();
            removed
        };
        self.persist().await?;
        Ok(removed)
    }

    /// Fold one settled round into a player's record.
    pub async fn record(&self, user: Uuid, outcome: RoundOutcome) -> Result<()> {
        {
            let mut guard = self.inner.lock().await;
            let stats = guard.users.entry(user).or_default();
            stats.rounds += 1;
            stats.hands += outcome.hands;
            stats.won += outcome.won;
            stats.lost += outcome.lost;
            stats.push += outcome.push;
            stats.naturals += outcome.naturals;
            stats.busts += outcome.busts;
            stats.doubles += outcome.doubles;
            stats.splits += outcome.splits;
            stats.insurance_taken += u64::from(outcome.insured);
            stats.wagered += outcome.wagered;
            stats.returned += outcome.returned;
        }
        self.persist().await
    }

    /// Seed the record from the bank ledger, once.
    ///
    /// Every round ever played left a `BlackjackBet` and, if it paid, a
    /// `BlackjackPayout` under one game id. Grouping by that id recovers the
    /// money exactly and the outcome roughly: paid more than staked is a win,
    /// exactly the stake is a push, nothing back is a loss. It cannot see a
    /// bust, a split or a double, which is what `derived_rounds` is for.
    pub async fn backfill_from_ledger(&self, accounts: &[Account]) -> Result<usize> {
        if self.inner.lock().await.backfilled_at.is_some() {
            return Ok(0);
        }
        let mut rounds = 0;
        {
            let mut guard = self.inner.lock().await;
            for account in accounts {
                let AccountOwner::User(user) = account.owner else {
                    // Only people play blackjack; the house has no table.
                    continue;
                };
                let mut games: HashMap<Uuid, (Cents, Cents)> = HashMap::new();
                for entry in &account.entries {
                    match entry.kind {
                        LedgerKind::BlackjackBet { game } => {
                            games.entry(game).or_default().0 += -entry.delta;
                        }
                        LedgerKind::BlackjackPayout { game } => {
                            games.entry(game).or_default().1 += entry.delta;
                        }
                        _ => {}
                    }
                }
                if games.is_empty() {
                    continue;
                }
                let stats = guard.users.entry(user).or_default();
                for (staked, returned) in games.into_values() {
                    if staked <= 0 {
                        continue;
                    }
                    rounds += 1;
                    stats.rounds += 1;
                    stats.derived_rounds += 1;
                    // One round, one hand: the ledger cannot tell a split
                    // apart from a doubled bet, so it counts as neither.
                    stats.hands += 1;
                    stats.wagered += staked;
                    stats.returned += returned;
                    match returned.cmp(&staked) {
                        std::cmp::Ordering::Greater => {
                            stats.won += 1;
                            // A natural pays exactly three for two.
                            if returned * 2 == staked * 5 {
                                stats.naturals += 1;
                            }
                        }
                        std::cmp::Ordering::Equal => stats.push += 1,
                        std::cmp::Ordering::Less => stats.lost += 1,
                    }
                }
            }
            guard.backfilled_at = Some(Utc::now());
        }
        self.persist().await?;
        Ok(rounds)
    }

    async fn persist(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let file = self.inner.lock().await.clone();
        let body = serde_json::to_vec_pretty(&file)?;
        let tmp = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
        tokio::fs::write(&tmp, body).await?;
        tokio::fs::rename(tmp, path).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank::BankStore;

    fn tempdir() -> PathBuf {
        std::env::temp_dir().join(format!("two-seven-bj-stats-{}", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn the_ledger_seeds_a_record_for_rounds_nobody_watched() {
        let bank = BankStore::load(tempdir()).await.unwrap();
        let user = Uuid::new_v4();
        let owner = AccountOwner::User(user);
        bank.re_up(owner.clone()).await.unwrap();

        // A win, a push, and a loss.
        let won = Uuid::new_v4();
        bank.blackjack_bet(owner.clone(), won, 1_000).await.unwrap();
        bank.blackjack_payout(owner.clone(), won, 2_000)
            .await
            .unwrap();
        let pushed = Uuid::new_v4();
        bank.blackjack_bet(owner.clone(), pushed, 500)
            .await
            .unwrap();
        bank.blackjack_payout(owner.clone(), pushed, 500)
            .await
            .unwrap();
        let lost = Uuid::new_v4();
        bank.blackjack_bet(owner.clone(), lost, 300).await.unwrap();

        let stats = BlackjackStatsStore::load(tempdir()).await.unwrap();
        let seeded = stats.backfill_from_ledger(&bank.accounts().await).await;
        assert_eq!(seeded.unwrap(), 3);

        let record = stats.of(user).await;
        assert_eq!(record.rounds, 3);
        assert_eq!((record.won, record.push, record.lost), (1, 1, 1));
        assert_eq!(record.wagered, 1_800);
        assert_eq!(record.returned, 2_500);
        assert_eq!(record.net(), 700);
        // Nothing here was watched, so no detail is claimed.
        assert_eq!(record.derived_rounds, 3);
        assert!(!record.watched());
    }

    #[tokio::test]
    async fn the_ledger_is_walked_once_however_often_it_is_asked() {
        let bank = BankStore::load(tempdir()).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        bank.re_up(owner.clone()).await.unwrap();
        bank.blackjack_bet(owner.clone(), Uuid::new_v4(), 1_000)
            .await
            .unwrap();

        let root = tempdir();
        let stats = BlackjackStatsStore::load(&root).await.unwrap();
        let accounts = bank.accounts().await;
        assert_eq!(stats.backfill_from_ledger(&accounts).await.unwrap(), 1);
        assert_eq!(stats.backfill_from_ledger(&accounts).await.unwrap(), 0);

        // And the watermark survives a restart, so a reboot cannot double it.
        let reloaded = BlackjackStatsStore::load(&root).await.unwrap();
        assert_eq!(reloaded.backfill_from_ledger(&accounts).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn a_watched_round_carries_the_detail_the_ledger_never_had() {
        let stats = BlackjackStatsStore::load(tempdir()).await.unwrap();
        let user = Uuid::new_v4();
        stats
            .record(
                user,
                RoundOutcome {
                    hands: 2,
                    won: 1,
                    lost: 1,
                    busts: 1,
                    splits: 1,
                    doubles: 1,
                    wagered: 3_000,
                    returned: 2_000,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let record = stats.of(user).await;
        assert_eq!(record.rounds, 1);
        assert_eq!(record.hands, 2);
        assert_eq!(record.busts, 1);
        assert_eq!(record.net(), -1_000);
        assert!(record.watched());
    }
}
