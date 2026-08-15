//! What every player has done at the tables.
//!
//! Keyed the same way as the bank, so a house regular is tracked exactly like
//! a person. Updated once per finished hand from the record the table hands
//! back, and read by the leaderboard.

use crate::{
    bank::AccountOwner,
    holdem::{HandEventKind, Street},
    money::Cents,
    table::{HandRecord, SeatOccupant},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlayerStats {
    pub hands: u64,
    /// Hands where they put money in before the flop by choice, blinds aside.
    pub voluntary: u64,
    /// Hands where they raised before the flop.
    pub raised_preflop: u64,
    pub hands_won: u64,
    pub showdowns: u64,
    pub showdowns_won: u64,
    pub biggest_pot: Cents,
    /// Chips won less chips put in, across every hand on record.
    pub net: Cents,
}

impl PlayerStats {
    pub fn vpip_percent(&self) -> u64 {
        (self.voluntary * 100).checked_div(self.hands).unwrap_or(0)
    }

    pub fn pfr_percent(&self) -> u64 {
        (self.raised_preflop * 100)
            .checked_div(self.hands)
            .unwrap_or(0)
    }

    pub fn win_percent(&self) -> u64 {
        (self.hands_won * 100).checked_div(self.hands).unwrap_or(0)
    }

    pub fn showdown_win_percent(&self) -> u64 {
        (self.showdowns_won * 100)
            .checked_div(self.showdowns)
            .unwrap_or(0)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct StatsFile {
    players: HashMap<String, PlayerStats>,
}

#[derive(Clone)]
pub struct StatsStore {
    inner: Arc<Mutex<HashMap<String, PlayerStats>>>,
    path: PathBuf,
}

/// The bank's owner key as a string, so the map serialises cleanly.
fn key(owner: &AccountOwner) -> String {
    match owner {
        AccountOwner::User(id) => format!("user:{id}"),
        AccountOwner::Bot(bot) => format!("bot:{bot}"),
    }
}

fn owner_of(occupant: &SeatOccupant) -> Option<AccountOwner> {
    match occupant {
        SeatOccupant::Human { user_id } => Some(AccountOwner::User(*user_id)),
        SeatOccupant::Bot { kind, seat } => {
            Some(AccountOwner::Bot(crate::table::Bot::new(*kind, *seat)))
        }
        SeatOccupant::Empty => None,
    }
}

impl StatsStore {
    pub async fn load(root: impl AsRef<Path>) -> Result<Self> {
        let dir = root.as_ref().join("stats");
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join("players.json");
        let players = match tokio::fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice::<StatsFile>(&bytes)
                .map(|file| file.players)
                .unwrap_or_default(),
            Err(_) => HashMap::new(),
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(players)),
            path,
        })
    }

    /// Forget everything the house has done. Paired with the bank's reset, so
    /// a fresh set of regulars starts with no money and no record.
    pub async fn forget_bots(&self) -> Result<()> {
        {
            let mut guard = self.inner.lock().await;
            let before = guard.len();
            guard.retain(|key, _| !key.starts_with("bot:"));
            if guard.len() == before {
                return Ok(());
            }
        }
        self.persist().await
    }

    pub async fn of(&self, owner: &AccountOwner) -> PlayerStats {
        self.inner
            .lock()
            .await
            .get(&key(owner))
            .copied()
            .unwrap_or_default()
    }

    pub async fn all(&self) -> HashMap<String, PlayerStats> {
        self.inner.lock().await.clone()
    }

    /// Fold one finished hand into everybody's record.
    pub async fn record(&self, hand: &HandRecord) -> Result<()> {
        let pot: Cents = hand.summary.awards.iter().map(|award| award.amount).sum();
        {
            let mut guard = self.inner.lock().await;
            for seat in &hand.seats {
                let Some(owner) = owner_of(&seat.occupant) else {
                    continue;
                };
                let stats = guard.entry(key(&owner)).or_default();
                stats.hands += 1;
                stats.net += seat.stack_after - seat.stack_before;
                let won: Cents = hand
                    .summary
                    .awards
                    .iter()
                    .filter(|award| award.seat == seat.seat)
                    .map(|award| award.amount)
                    .sum();
                if won > 0 {
                    stats.hands_won += 1;
                    stats.biggest_pot = stats.biggest_pot.max(pot);
                }
                let showed = hand
                    .summary
                    .revealed_hole_cards
                    .iter()
                    .any(|(index, _)| *index == seat.seat);
                if showed {
                    stats.showdowns += 1;
                    if won > 0 {
                        stats.showdowns_won += 1;
                    }
                }
                // Blinds and antes are posted, not chosen; everything else
                // before the flop is money they decided to put in.
                let preflop = hand.summary.events.iter().filter(|event| {
                    event.street == Street::Preflop && event.seat == Some(seat.seat)
                });
                let mut voluntary = false;
                let mut raised = false;
                for event in preflop {
                    match event.kind {
                        HandEventKind::Call | HandEventKind::Bet | HandEventKind::AllIn => {
                            voluntary = true;
                        }
                        HandEventKind::Raise => {
                            voluntary = true;
                            raised = true;
                        }
                        _ => {}
                    }
                }
                if voluntary {
                    stats.voluntary += 1;
                }
                if raised {
                    stats.raised_preflop += 1;
                }
            }
        }
        self.persist().await
    }

    async fn persist(&self) -> Result<()> {
        let players = self.inner.lock().await.clone();
        let body = serde_json::to_vec_pretty(&StatsFile { players })?;
        let tmp = self
            .path
            .with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
        tokio::fs::write(&tmp, body).await?;
        tokio::fs::rename(tmp, &self.path).await?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_house_reset_forgets_their_record_but_not_ours() {
        let root = std::env::temp_dir().join(format!("two-seven-stats-{}", uuid::Uuid::new_v4()));
        let stats = StatsStore::load(&root).await.unwrap();
        let bot = AccountOwner::Bot(crate::table::Bot::new(crate::table::BotKind::Fish, 0));
        let person = AccountOwner::User(uuid::Uuid::new_v4());
        {
            let mut guard = stats.inner.lock().await;
            guard.insert(
                key(&bot),
                PlayerStats {
                    hands: 12,
                    ..Default::default()
                },
            );
            guard.insert(
                key(&person),
                PlayerStats {
                    hands: 7,
                    ..Default::default()
                },
            );
        }
        stats.forget_bots().await.unwrap();
        assert_eq!(stats.of(&bot).await, PlayerStats::default());
        assert_eq!(stats.of(&person).await.hands, 7);

        // And it stays forgotten across a reload.
        let reloaded = StatsStore::load(&root).await.unwrap();
        assert_eq!(reloaded.of(&bot).await, PlayerStats::default());
        assert_eq!(reloaded.of(&person).await.hands, 7);
    }
}
