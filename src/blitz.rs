use crate::{
    cards::{Card, Deck},
    eval::{EvaluatedHand, evaluate},
    money::Cents,
};
use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BlitzDifficulty {
    Easy,
    Normal,
    Hard,
}

impl BlitzDifficulty {
    pub const ALL: [Self; 3] = [Self::Easy, Self::Normal, Self::Hard];

    pub fn config(self) -> BlitzDifficultyConfig {
        match self {
            Self::Easy => BlitzDifficultyConfig {
                id: "easy",
                label: "Easy",
                time_limit_ms: 20_000,
                buy_in: 100,
            },
            Self::Normal => BlitzDifficultyConfig {
                id: "normal",
                label: "Normal",
                time_limit_ms: 12_000,
                buy_in: 500,
            },
            Self::Hard => BlitzDifficultyConfig {
                id: "hard",
                label: "Hard",
                time_limit_ms: 6_000,
                buy_in: 2_000,
            },
        }
    }
}

impl std::fmt::Display for BlitzDifficulty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.config().id)
    }
}

impl FromStr for BlitzDifficulty {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "easy" | "Easy" => Ok(Self::Easy),
            "normal" | "Normal" => Ok(Self::Normal),
            "hard" | "Hard" => Ok(Self::Hard),
            _ => Err(format!("unknown hand blitz difficulty {value}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct BlitzDifficultyConfig {
    pub id: &'static str,
    pub label: &'static str,
    pub time_limit_ms: u64,
    pub buy_in: Cents,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlitzStats {
    pub runs: u64,
    pub attempts: u64,
    pub correct: u64,
    pub total_answer_ms: u64,
    pub best_streak: u64,
}

impl BlitzStats {
    pub fn accuracy_percent(&self) -> u64 {
        (self.correct * 100).checked_div(self.attempts).unwrap_or(0)
    }

    pub fn avg_answer_ms(&self) -> u64 {
        self.total_answer_ms.checked_div(self.correct).unwrap_or(0)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct BlitzRoundView {
    pub id: Uuid,
    pub board: Vec<Card>,
    pub hands: [Vec<Card>; 2],
    pub time_limit_ms: u64,
    pub deadline_ms: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlitzRunView {
    pub id: Uuid,
    pub difficulty: BlitzDifficultyConfig,
    pub active: bool,
    pub correct: u64,
    pub earnings: Cents,
    pub next_payout: Cents,
    pub round: BlitzRoundView,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlitzAnswerResult {
    pub correct: bool,
    pub active: bool,
    pub timed_out: bool,
    pub payout_awarded: Cents,
    pub winner: usize,
    pub winning_label: String,
    pub answer_ms: u64,
    pub run: BlitzRunView,
    pub stats: BlitzStats,
}

#[derive(Clone)]
pub struct BlitzStore {
    inner: Arc<Mutex<Inner>>,
    path: PathBuf,
}

#[derive(Default, Serialize, Deserialize)]
struct StatsFile {
    users: HashMap<Uuid, BlitzStats>,
}

#[derive(Default)]
struct Inner {
    runs: HashMap<Uuid, BlitzRun>,
    stats: HashMap<Uuid, BlitzStats>,
}

#[derive(Clone, Debug)]
struct BlitzRun {
    id: Uuid,
    user: Uuid,
    difficulty: BlitzDifficulty,
    buy_in: Cents,
    base_time_ms: u64,
    active: bool,
    correct: u64,
    earnings: Cents,
    round: BlitzRound,
}

#[derive(Clone, Debug)]
struct BlitzRound {
    id: Uuid,
    board: Vec<Card>,
    hands: [Vec<Card>; 2],
    ranks: [EvaluatedHand; 2],
    winner: usize,
    dealt_at: DateTime<Utc>,
    time_limit_ms: u64,
}

impl BlitzStore {
    pub async fn load(root: impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let dir = root.as_ref().join("blitz");
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join("stats.json");
        let stats = match tokio::fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice::<StatsFile>(&bytes)?.users,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                runs: HashMap::new(),
                stats,
            })),
            path,
        })
    }

    pub async fn stats(&self, user: Uuid) -> BlitzStats {
        self.inner
            .lock()
            .await
            .stats
            .get(&user)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn start(&self, user: Uuid, difficulty: BlitzDifficulty, id: Uuid) -> BlitzRunView {
        let config = difficulty.config();
        let mut guard = self.inner.lock().await;
        let stats = guard.stats.entry(user).or_default();
        stats.runs += 1;
        let run = BlitzRun {
            id,
            user,
            difficulty,
            buy_in: config.buy_in,
            base_time_ms: config.time_limit_ms,
            active: true,
            correct: 0,
            earnings: 0,
            round: deal_round(config.time_limit_ms),
        };
        let view = run.view();
        guard.runs.insert(id, run);
        view
    }

    pub async fn answer(
        &self,
        user: Uuid,
        run_id: Uuid,
        round_id: Uuid,
        choice: usize,
    ) -> Result<BlitzAnswerResult, BlitzAnswerError> {
        let mut guard = self.inner.lock().await;
        let mut run = guard
            .runs
            .remove(&run_id)
            .ok_or(BlitzAnswerError::NotFound)?;
        if run.user != user {
            guard.runs.insert(run_id, run);
            return Err(BlitzAnswerError::NotFound);
        }
        if !run.active || run.round.id != round_id || choice > 1 {
            guard.runs.insert(run_id, run);
            return Err(BlitzAnswerError::Unavailable);
        }
        let now = Utc::now();
        let answer_ms = now
            .signed_duration_since(run.round.dealt_at)
            .num_milliseconds()
            .max(0) as u64;
        let timed_out = answer_ms > run.round.time_limit_ms;
        let correct = !timed_out && choice == run.round.winner;
        let winner = run.round.winner;
        let winning_label = run.round.ranks[winner].label.clone();
        let payout_awarded = if correct { run.next_payout() } else { 0 };
        {
            let stats = guard.stats.entry(user).or_default();
            stats.attempts += 1;
            if correct {
                stats.correct += 1;
                stats.total_answer_ms += answer_ms;
                stats.best_streak = stats.best_streak.max(run.correct + 1);
            }
        }
        if correct {
            run.correct += 1;
            run.earnings += payout_awarded;
            run.round = deal_round(run.current_time_limit_ms());
        } else {
            run.active = false;
        }
        let stats = guard.stats.get(&user).cloned().unwrap_or_default();
        let result = BlitzAnswerResult {
            correct,
            active: run.active,
            timed_out,
            payout_awarded,
            winner,
            winning_label,
            answer_ms,
            run: run.view(),
            stats,
        };
        guard.runs.insert(run_id, run);
        Ok(result)
    }

    pub async fn persist_stats(&self) -> Result<(), anyhow::Error> {
        let users = self.inner.lock().await.stats.clone();
        let data = serde_json::to_vec_pretty(&StatsFile { users })?;
        let tmp = self.path.with_extension(format!("tmp-{}", Uuid::new_v4()));
        tokio::fs::write(&tmp, data).await?;
        tokio::fs::rename(tmp, &self.path).await?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlitzAnswerError {
    NotFound,
    Unavailable,
}

impl BlitzRun {
    fn current_time_limit_ms(&self) -> u64 {
        let mut limit = self.base_time_ms;
        let mut threshold = self.buy_in * 2;
        while self.earnings >= threshold {
            limit = (limit / 2).max(500);
            threshold *= 2;
        }
        limit
    }

    fn view(&self) -> BlitzRunView {
        BlitzRunView {
            id: self.id,
            difficulty: self.difficulty.config(),
            active: self.active,
            correct: self.correct,
            earnings: self.earnings,
            next_payout: self.next_payout(),
            round: self.round.view(),
        }
    }

    fn next_payout(&self) -> Cents {
        let next_correct = self.correct + 1;
        (self.buy_in * next_correct as Cents / 3 - self.earnings).max(1)
    }
}

impl BlitzRound {
    fn view(&self) -> BlitzRoundView {
        BlitzRoundView {
            id: self.id,
            board: self.board.clone(),
            hands: self.hands.clone(),
            time_limit_ms: self.time_limit_ms,
            deadline_ms: self.dealt_at.timestamp_millis() + self.time_limit_ms as i64,
        }
    }
}

fn deal_round(time_limit_ms: u64) -> BlitzRound {
    loop {
        let mut deck = Deck::seeded(rand::thread_rng().r#gen());
        let hands = [
            vec![deck.deal().expect("card"), deck.deal().expect("card")],
            vec![deck.deal().expect("card"), deck.deal().expect("card")],
        ];
        let board: Vec<Card> = (0..5).map(|_| deck.deal().expect("card")).collect();
        let ranks = [
            evaluate(&[hands[0].clone(), board.clone()].concat()),
            evaluate(&[hands[1].clone(), board.clone()].concat()),
        ];
        if ranks[0].rank == ranks[1].rank {
            continue;
        }
        return BlitzRound {
            id: Uuid::new_v4(),
            board,
            hands,
            winner: usize::from(ranks[1].rank > ranks[0].rank),
            ranks,
            dealt_at: Utc::now(),
            time_limit_ms,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_limit_halves_at_earnings_doubling_thresholds() {
        let mut run = BlitzRun {
            id: Uuid::new_v4(),
            user: Uuid::new_v4(),
            difficulty: BlitzDifficulty::Easy,
            buy_in: 300,
            base_time_ms: 8_000,
            active: true,
            correct: 0,
            earnings: 1_199,
            round: deal_round(8_000),
        };
        assert_eq!(run.current_time_limit_ms(), 4_000);
        run.earnings = 1_200;
        assert_eq!(run.current_time_limit_ms(), 2_000);
    }

    #[test]
    fn payouts_distribute_cents_to_exact_thirds() {
        let mut run = BlitzRun {
            id: Uuid::new_v4(),
            user: Uuid::new_v4(),
            difficulty: BlitzDifficulty::Easy,
            buy_in: 100,
            base_time_ms: 8_000,
            active: true,
            correct: 0,
            earnings: 0,
            round: deal_round(8_000),
        };
        assert_eq!(run.next_payout(), 33);
        run.correct = 1;
        run.earnings = 33;
        assert_eq!(run.next_payout(), 33);
        run.correct = 2;
        run.earnings = 66;
        assert_eq!(run.next_payout(), 34);
    }

    #[test]
    fn dealt_round_has_one_winner() {
        let round = deal_round(5_000);
        assert_ne!(round.ranks[0].rank, round.ranks[1].rank);
        assert!(round.winner <= 1);
    }
}
