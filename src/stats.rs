//! What every player has done at the tables.
//!
//! Keyed the same way as the bank, so a house regular is tracked exactly like
//! a person. Updated once per finished hand from the record the table hands
//! back, and read by the leaderboard.

use crate::{
    bank::AccountOwner,
    cards::Card,
    eval::Category,
    holdem::{HandEventKind, Street},
    money::Cents,
    table::{HandRecord, SeatOccupant},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
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
    /// Winning hands whose make is known, by category. A hand everyone folded
    /// to never turns over, so it has no category and is counted nowhere here:
    /// these sum to `wins_shown`, never to `hands_won`.
    #[serde(default)]
    pub won_by_category: BTreeMap<Category, u64>,
    /// Winning hands that reached a showdown, so the sum of the map above.
    #[serde(default)]
    pub wins_shown: u64,
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

    pub fn won_with(&self, category: Category) -> u64 {
        self.won_by_category.get(&category).copied().unwrap_or(0)
    }

    /// How much of their winning form that category is, in tenths of a percent
    /// so a hand as rare as quads doesn't round away to nothing.
    pub fn won_with_permille(&self, category: Category) -> u64 {
        (self.won_with(category) * 1_000)
            .checked_div(self.wins_shown)
            .unwrap_or(0)
    }
}

/// A hand rare enough to keep forever: quads or better, with the money it took
/// down. Kept keyed by owner rather than by name, so a rename follows through
/// to the record book.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BigHand {
    pub at: DateTime<Utc>,
    pub table: Uuid,
    pub hand_no: u64,
    /// The bank's owner key, resolved to a name when the board is rendered.
    pub owner: String,
    pub category: Category,
    /// A straight flush to the ace. Flagged rather than given its own board:
    /// there may never be more than one.
    pub royal: bool,
    pub label: String,
    /// The five cards that made it, best first.
    pub cards: Vec<Card>,
    pub won: Cents,
}

/// A hand where the money went in before the board was finished and the
/// favourite lost. One record serves both beat boards.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Beat {
    pub at: DateTime<Utc>,
    pub table: Uuid,
    pub hand_no: u64,
    pub loser: String,
    /// The loser's equity when the last chips went in, in tenths of a percent.
    pub loser_equity_permille: u16,
    pub loser_label: String,
    pub winner: String,
    pub winner_equity_permille: u16,
    pub winner_label: String,
    /// What the hand paid the player who won it.
    pub pot: Cents,
}

/// A favourite this far ahead who still lost is what the worst-beats board is
/// for (§ leaderboard): 95% and up, ranked by the money rather than the odds.
pub const COOLER_PERMILLE: u16 = 950;
/// Below this the loser was never really a favourite, so the hand is not worth
/// keeping. Comfortably under the boards' own cut-offs, so neither is starved.
const BEAT_FLOOR_PERMILLE: u16 = 700;
/// How many of each kind the record books keep. Both beat boards are computed
/// exactly from what survives, never approximated.
const KEEP_BEATS: usize = 200;
const KEEP_BIG_HANDS: usize = 500;

#[derive(Clone, Default, Serialize, Deserialize)]
struct StatsFile {
    players: HashMap<String, PlayerStats>,
    /// Quads and better, all-time.
    #[serde(default)]
    big_hands: Vec<BigHand>,
    /// Favourites who lost with the board still to come.
    #[serde(default)]
    beats: Vec<Beat>,
    /// Set once the record books have been rebuilt from the hand history, so
    /// the walk over every hand ever played happens exactly once.
    #[serde(default)]
    backfilled_at: Option<DateTime<Utc>>,
    /// Set once the winning-hand breakdown has been recounted from the hand
    /// history. Its own mark: the books were rebuilt before that breakdown
    /// existed, so a tree carrying the first mark still needs this walk.
    #[serde(default)]
    categories_backfilled_at: Option<DateTime<Utc>>,
}

#[derive(Clone)]
pub struct StatsStore {
    inner: Arc<Mutex<StatsFile>>,
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
        let file = match tokio::fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice::<StatsFile>(&bytes).unwrap_or_default(),
            Err(_) => StatsFile::default(),
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(file)),
            path,
        })
    }

    /// Forget everything the house has done. Paired with the bank's reset, so
    /// a fresh set of regulars starts with no money and no record. The record
    /// books go too: a regular who no longer exists should not still be
    /// holding the straight flush.
    pub async fn forget_bots(&self) -> Result<()> {
        {
            let mut guard = self.inner.lock().await;
            let before = (
                guard.players.len(),
                guard.big_hands.len(),
                guard.beats.len(),
            );
            guard.players.retain(|key, _| !key.starts_with("bot:"));
            guard
                .big_hands
                .retain(|hand| !hand.owner.starts_with("bot:"));
            guard
                .beats
                .retain(|beat| !beat.loser.starts_with("bot:") && !beat.winner.starts_with("bot:"));
            if before
                == (
                    guard.players.len(),
                    guard.big_hands.len(),
                    guard.beats.len(),
                )
            {
                return Ok(());
            }
        }
        self.persist().await
    }

    pub async fn of(&self, owner: &AccountOwner) -> PlayerStats {
        self.inner
            .lock()
            .await
            .players
            .get(&key(owner))
            .cloned()
            .unwrap_or_default()
    }

    pub async fn all(&self) -> HashMap<String, PlayerStats> {
        self.inner.lock().await.players.clone()
    }

    /// Quads and better, best hand first, then newest.
    pub async fn big_hands(&self) -> Vec<BigHand> {
        let mut hands = self.inner.lock().await.big_hands.clone();
        hands.sort_by(|left, right| {
            right
                .category
                .cmp(&left.category)
                .then(right.won.cmp(&left.won))
                .then(right.at.cmp(&left.at))
        });
        hands
    }

    /// Every beat on record. The two boards are cut from this by the caller.
    pub async fn beats(&self) -> Vec<Beat> {
        self.inner.lock().await.beats.clone()
    }

    pub async fn reset_all(&self) -> Result<usize> {
        let removed = {
            let mut guard = self.inner.lock().await;
            let removed = guard.players.len();
            // A reset that left the record books standing would credit hands
            // to players the leaderboard no longer knows.
            *guard = StatsFile {
                backfilled_at: guard.backfilled_at,
                categories_backfilled_at: guard.categories_backfilled_at,
                ..StatsFile::default()
            };
            removed
        };
        self.persist().await?;
        Ok(removed)
    }

    /// Fold one finished hand into everybody's record.
    pub async fn record(&self, hand: &HandRecord) -> Result<()> {
        {
            let mut guard = self.inner.lock().await;
            fold(&mut guard, hand);
            prune(&mut guard);
        }
        self.persist().await
    }

    /// Rebuild the record books from every hand ever played, once.
    ///
    /// The straight flushes and the beats were always in the history file;
    /// nothing recorded them until now. Walking it at boot means the boards
    /// open full instead of waiting years for a royal. The player tallies are
    /// left alone — those have been kept correctly all along, and replaying
    /// them would double every number.
    pub async fn backfill_records(&self, history: &crate::history::HistoryStore) -> Result<usize> {
        if self.inner.lock().await.backfilled_at.is_some() {
            return Ok(0);
        }
        let mut found = 0;
        {
            let mut guard = self.inner.lock().await;
            guard.big_hands.clear();
            guard.beats.clear();
            for hand in history.every_hand().await {
                let before = (guard.big_hands.len(), guard.beats.len());
                fold_records(&mut guard, &hand);
                if before != (guard.big_hands.len(), guard.beats.len()) {
                    found += 1;
                }
            }
            prune(&mut guard);
            guard.backfilled_at = Some(Utc::now());
        }
        self.persist().await?;
        Ok(found)
    }

    /// Recount the winning-hand breakdown from every hand ever played, once.
    ///
    /// `won_by_category` and `wins_shown` arrived after the tallies did, so
    /// every record written before them counts nothing under a make it
    /// certainly had. Unlike the other tallies these are recoverable: the
    /// history says what each winner turned over. The walk therefore replaces
    /// the two fields outright rather than adding to them — the hands it reads
    /// include the ones already counted, so adding would double them — and
    /// leaves every other number alone.
    pub async fn backfill_categories(
        &self,
        history: &crate::history::HistoryStore,
    ) -> Result<usize> {
        if self.inner.lock().await.categories_backfilled_at.is_some() {
            return Ok(0);
        }
        let mut counted = 0;
        {
            let mut guard = self.inner.lock().await;
            for stats in guard.players.values_mut() {
                stats.won_by_category.clear();
                stats.wins_shown = 0;
            }
            for hand in history.every_hand().await {
                for seat in &hand.seats {
                    let Some(owner) = owner_of(&seat.occupant) else {
                        continue;
                    };
                    let won: Cents = hand
                        .summary
                        .awards
                        .iter()
                        .filter(|award| award.seat == seat.seat)
                        .map(|award| award.amount)
                        .sum();
                    if won == 0 {
                        continue;
                    }
                    let Some(made) = made_hand(&hand, seat.seat) else {
                        continue;
                    };
                    let category = made.rank.category;
                    // A player the tallies never knew is not invented here:
                    // this walk fixes a breakdown, it does not add records.
                    let Some(stats) = guard.players.get_mut(&key(&owner)) else {
                        continue;
                    };
                    stats.wins_shown += 1;
                    *stats.won_by_category.entry(category).or_default() += 1;
                    counted += 1;
                }
            }
            guard.categories_backfilled_at = Some(Utc::now());
        }
        self.persist().await?;
        Ok(counted)
    }

    async fn persist(&self) -> Result<()> {
        let file = self.inner.lock().await.clone();
        let body = serde_json::to_vec_pretty(&file)?;
        let tmp = self
            .path
            .with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
        tokio::fs::write(&tmp, body).await?;
        tokio::fs::rename(tmp, &self.path).await?;
        Ok(())
    }
}

/// Everything one finished hand adds: the per-player tallies, then the two
/// record books.
fn fold(file: &mut StatsFile, hand: &HandRecord) {
    let pot: Cents = hand.summary.awards.iter().map(|award| award.amount).sum();
    for seat in &hand.seats {
        let Some(owner) = owner_of(&seat.occupant) else {
            continue;
        };
        let stats = file.players.entry(key(&owner)).or_default();
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
            // A hand everyone folded to never turns over, so only a winner
            // who showed has a make to count.
            if let Some(made) = made_hand(hand, seat.seat) {
                stats.wins_shown += 1;
                *stats.won_by_category.entry(made.rank.category).or_default() += 1;
            }
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
        let preflop = hand
            .summary
            .events
            .iter()
            .filter(|event| event.street == Street::Preflop && event.seat == Some(seat.seat));
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
    fold_records(file, hand);
}

/// The two record books only. Split out because the backfill replays these
/// over the whole history, where replaying the tallies would double them.
fn fold_records(file: &mut StatsFile, hand: &HandRecord) {
    let owner_at = |seat: usize| -> Option<String> {
        hand.seats
            .iter()
            .find(|record| record.seat == seat)
            .and_then(|record| owner_of(&record.occupant))
            .map(|owner| key(&owner))
    };
    let won_by = |seat: usize| -> Cents {
        hand.summary
            .awards
            .iter()
            .filter(|award| award.seat == seat)
            .map(|award| award.amount)
            .sum()
    };

    for result in &hand.summary.results {
        let Some(made) = result.hand.as_ref() else {
            continue;
        };
        if !matches!(
            made.rank.category,
            Category::FourOfAKind | Category::StraightFlush
        ) {
            continue;
        }
        let Some(owner) = owner_at(result.seat) else {
            continue;
        };
        file.big_hands.push(BigHand {
            at: hand.at,
            table: hand.table,
            hand_no: hand.hand_no,
            owner,
            category: made.rank.category,
            royal: made.rank.category == Category::StraightFlush
                && made.rank.kickers.first() == Some(&14),
            label: made.label.clone(),
            cards: made.cards.clone(),
            won: won_by(result.seat),
        });
    }

    if let Some(beat) = beat_in(hand) {
        file.beats.push(beat);
    }
}

/// The favourite who lost, if this hand had one.
///
/// Equity only means anything while the board is unfinished: `reveal_odds` is
/// measured at `runout_from`, so a hand that was bet all the way to the river
/// has every player already at 100% or 0% and can never be a beat. That is
/// also exactly the reading asked for — the chance of winning *when the last
/// bets were made*.
fn beat_in(hand: &HandRecord) -> Option<Beat> {
    let summary = &hand.summary;
    if summary.runout_from >= summary.board.len() || summary.reveal_odds.len() < 2 {
        return None;
    }
    let owner_at = |seat: usize| -> Option<String> {
        hand.seats
            .iter()
            .find(|record| record.seat == seat)
            .and_then(|record| owner_of(&record.occupant))
            .map(|owner| key(&owner))
    };
    let won_by = |seat: usize| -> Cents {
        summary
            .awards
            .iter()
            .filter(|award| award.seat == seat)
            .map(|award| award.amount)
            .sum()
    };
    let label_at = |seat: usize| -> String {
        summary
            .results
            .iter()
            .find(|result| result.seat == seat)
            .and_then(|result| result.hand.as_ref())
            .map(|made| made.label.clone())
            .unwrap_or_default()
    };

    // The biggest favourite who took nothing, and the winner who was furthest
    // behind them. A chopped pot beats nobody, so a seat that got paid at all
    // is not the loser.
    let loser = summary
        .reveal_odds
        .iter()
        .filter(|odds| won_by(odds.seat) == 0)
        .max_by_key(|odds| odds.equity_permille)?;
    if loser.equity_permille < BEAT_FLOOR_PERMILLE {
        return None;
    }
    let winner = summary
        .reveal_odds
        .iter()
        .filter(|odds| won_by(odds.seat) > 0)
        .min_by_key(|odds| odds.equity_permille)?;
    if winner.equity_permille >= loser.equity_permille {
        return None;
    }
    Some(Beat {
        at: hand.at,
        table: hand.table,
        hand_no: hand.hand_no,
        loser: owner_at(loser.seat)?,
        loser_equity_permille: loser.equity_permille,
        loser_label: label_at(loser.seat),
        winner: owner_at(winner.seat)?,
        winner_equity_permille: winner.equity_permille,
        winner_label: label_at(winner.seat),
        pot: won_by(winner.seat),
    })
}

/// What a seat turned over, if it reached a showdown.
fn made_hand(hand: &HandRecord, seat: usize) -> Option<&crate::eval::EvaluatedHand> {
    hand.summary
        .results
        .iter()
        .find(|result| result.seat == seat)
        .and_then(|result| result.hand.as_ref())
}

/// Keep the record books from growing without bound.
///
/// Both beat boards are cut from the same list, so the trim keeps the union of
/// what each of them can show: the biggest favourites, and the biggest pots
/// lost by a cooler-sized favourite. Anything neither board could ever reach
/// is dropped, which makes both of them exact rather than approximate.
fn prune(file: &mut StatsFile) {
    if file.big_hands.len() > KEEP_BIG_HANDS {
        file.big_hands.sort_by(|left, right| {
            right
                .category
                .cmp(&left.category)
                .then(right.won.cmp(&left.won))
                .then(right.at.cmp(&left.at))
        });
        file.big_hands.truncate(KEEP_BIG_HANDS);
    }

    file.beats
        .retain(|beat| beat.loser_equity_permille >= BEAT_FLOOR_PERMILLE);
    if file.beats.len() <= KEEP_BEATS * 2 {
        return;
    }
    let mut by_odds = file.beats.clone();
    by_odds.sort_by(|left, right| {
        right
            .loser_equity_permille
            .cmp(&left.loser_equity_permille)
            .then(right.pot.cmp(&left.pot))
    });
    by_odds.truncate(KEEP_BEATS);

    let mut by_money: Vec<Beat> = file
        .beats
        .iter()
        .filter(|beat| beat.loser_equity_permille >= COOLER_PERMILLE)
        .cloned()
        .collect();
    by_money.sort_by_key(|beat| std::cmp::Reverse(beat.pot));
    by_money.truncate(KEEP_BEATS);

    for beat in by_money {
        if !by_odds.contains(&beat) {
            by_odds.push(beat);
        }
    }
    file.beats = by_odds;
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
            guard.players.insert(
                key(&bot),
                PlayerStats {
                    hands: 12,
                    ..Default::default()
                },
            );
            guard.players.insert(
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

    fn cards(text: &str) -> Vec<Card> {
        text.as_bytes()
            .chunks(2)
            .map(|pair| std::str::from_utf8(pair).unwrap().parse().unwrap())
            .collect()
    }

    /// A finished hand: two seats all-in, a board, and whatever each turned
    /// over. `runout_from` is what decides whether the equity means anything.
    fn hand(
        seats: [(usize, Uuid, &str); 2],
        board: &str,
        runout_from: usize,
        winner: usize,
        pot: Cents,
        odds: [(usize, u16); 2],
    ) -> HandRecord {
        let board = cards(board);
        HandRecord {
            table: Uuid::new_v4(),
            hand_no: 1,
            at: Utc::now(),
            stakes: crate::table::Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            },
            button: 0,
            seats: seats
                .iter()
                .map(|(seat, user, hole)| crate::table::HandRecordSeat {
                    seat: *seat,
                    occupant: SeatOccupant::Human { user_id: *user },
                    hole_cards: cards(hole),
                    stack_before: 10_000,
                    stack_after: if *seat == winner { 10_000 + pot } else { 0 },
                })
                .collect(),
            summary: crate::holdem::HandSummary {
                results: seats
                    .iter()
                    .map(|(seat, _, hole)| crate::holdem::SeatResult {
                        seat: *seat,
                        hand: Some(crate::eval::evaluate(
                            &cards(hole)
                                .into_iter()
                                .chain(board.clone())
                                .collect::<Vec<_>>(),
                        )),
                    })
                    .collect(),
                awards: vec![crate::holdem::Award {
                    seat: winner,
                    amount: pot,
                }],
                contributions: seats.iter().map(|(seat, _, _)| (*seat, pot / 2)).collect(),
                revealed_hole_cards: seats
                    .iter()
                    .map(|(seat, _, hole)| (*seat, cards(hole)))
                    .collect(),
                events: Vec::new(),
                runout_from,
                runout: Vec::new(),
                stacks_before_awards: Default::default(),
                reveal_leaders: Vec::new(),
                reveal_odds: odds
                    .iter()
                    .map(|(seat, equity)| crate::holdem::ShowdownOdds {
                        seat: *seat,
                        equity_permille: *equity,
                        outs: Vec::new(),
                    })
                    .collect(),
                board,
            },
        }
    }

    #[tokio::test]
    async fn a_royal_lands_in_the_record_book_flagged_with_what_it_won() {
        let root = std::env::temp_dir().join(format!("two-seven-stats-{}", Uuid::new_v4()));
        let stats = StatsStore::load(&root).await.unwrap();
        let hero = Uuid::new_v4();
        let villain = Uuid::new_v4();
        // Hero holds the two hearts that finish a royal; villain has quads.
        let record = hand(
            [(0, hero, "AhKh"), (1, villain, "7s7d")],
            "QhJhTh7h7c",
            3,
            0,
            50_000,
            [(0, 977), (1, 23)],
        );
        stats.record(&record).await.unwrap();

        let books = stats.big_hands().await;
        // Both the straight flush and the quads are worth keeping.
        assert_eq!(books.len(), 2);
        let best = &books[0];
        assert_eq!(best.category, Category::StraightFlush);
        assert!(best.royal, "an ace-high straight flush is a royal");
        assert_eq!(best.won, 50_000);
        assert_eq!(best.owner, format!("user:{hero}"));
        assert_eq!(books[1].category, Category::FourOfAKind);
        // The loser made quads and took nothing.
        assert_eq!(books[1].won, 0);

        // And the winner's make is counted against the hands they showed.
        let poker = stats.of(&AccountOwner::User(hero)).await;
        assert_eq!(poker.wins_shown, 1);
        assert_eq!(poker.won_with(Category::StraightFlush), 1);
        assert_eq!(poker.won_with_permille(Category::StraightFlush), 1_000);
    }

    #[tokio::test]
    async fn a_beat_is_kept_from_both_ends_but_only_while_the_board_was_live() {
        let root = std::env::temp_dir().join(format!("two-seven-stats-{}", Uuid::new_v4()));
        let stats = StatsStore::load(&root).await.unwrap();
        let favourite = Uuid::new_v4();
        let winner = Uuid::new_v4();
        stats
            .record(&hand(
                [(0, favourite, "7s7d"), (1, winner, "AhKh")],
                "QhJhTh7h7c",
                3,
                1,
                50_000,
                [(0, 977), (1, 23)],
            ))
            .await
            .unwrap();

        let beats = stats.beats().await;
        assert_eq!(beats.len(), 1);
        assert_eq!(beats[0].loser, format!("user:{favourite}"));
        assert_eq!(beats[0].loser_equity_permille, 977);
        assert_eq!(beats[0].winner, format!("user:{winner}"));
        assert_eq!(beats[0].winner_equity_permille, 23);
        assert_eq!(beats[0].pot, 50_000);
        assert!(beats[0].loser_equity_permille >= COOLER_PERMILLE);
    }

    #[tokio::test]
    async fn a_hand_bet_to_the_river_is_never_a_beat() {
        let root = std::env::temp_dir().join(format!("two-seven-stats-{}", Uuid::new_v4()));
        let stats = StatsStore::load(&root).await.unwrap();
        // `runout_from` at the full board means nothing was left to come, so
        // the equity is only a restatement of the result.
        stats
            .record(&hand(
                [(0, Uuid::new_v4(), "7s7d"), (1, Uuid::new_v4(), "AhKh")],
                "QhJhTh7h7c",
                5,
                1,
                50_000,
                [(0, 0), (1, 1_000)],
            ))
            .await
            .unwrap();
        assert!(stats.beats().await.is_empty());
    }

    #[tokio::test]
    async fn a_narrow_favourite_who_loses_is_not_worth_a_record() {
        let root = std::env::temp_dir().join(format!("two-seven-stats-{}", Uuid::new_v4()));
        let stats = StatsStore::load(&root).await.unwrap();
        stats
            .record(&hand(
                [(0, Uuid::new_v4(), "7s7d"), (1, Uuid::new_v4(), "AhKh")],
                "QhJhTh7h7c",
                3,
                1,
                50_000,
                [(0, 550), (1, 450)],
            ))
            .await
            .unwrap();
        assert!(stats.beats().await.is_empty());
    }

    #[tokio::test]
    async fn the_history_is_walked_once_and_leaves_the_tallies_alone() {
        let root = std::env::temp_dir().join(format!("two-seven-backfill-{}", Uuid::new_v4()));
        let history = crate::history::HistoryStore::load(&root).await.unwrap();
        let hero = Uuid::new_v4();
        let record = hand(
            [(0, hero, "AhKh"), (1, Uuid::new_v4(), "7s7d")],
            "QhJhTh7h7c",
            3,
            0,
            50_000,
            [(0, 977), (1, 23)],
        );
        history.append(record.table, &record).await.unwrap();

        let stats = StatsStore::load(&root).await.unwrap();
        assert_eq!(stats.backfill_records(&history).await.unwrap(), 1);
        assert_eq!(stats.big_hands().await.len(), 2);
        // The player tallies were already kept correctly hand by hand, so the
        // walk must not touch them or every number would double.
        assert_eq!(stats.of(&AccountOwner::User(hero)).await.hands, 0);

        // A second boot finds the watermark and does nothing.
        assert_eq!(stats.backfill_records(&history).await.unwrap(), 0);
        let reloaded = StatsStore::load(&root).await.unwrap();
        assert_eq!(reloaded.backfill_records(&history).await.unwrap(), 0);
        assert_eq!(reloaded.big_hands().await.len(), 2);
    }

    #[tokio::test]
    async fn the_winning_hand_types_are_recounted_from_the_history_once() {
        let root = std::env::temp_dir().join(format!("two-seven-types-{}", Uuid::new_v4()));
        let history = crate::history::HistoryStore::load(&root).await.unwrap();
        let hero = Uuid::new_v4();
        let record = hand(
            [(0, hero, "AhKh"), (1, Uuid::new_v4(), "7s7d")],
            "QhJhTh2c3d",
            3,
            0,
            50_000,
            [(0, 977), (1, 23)],
        );
        history.append(record.table, &record).await.unwrap();

        // A record kept before the breakdown existed: the hand is counted,
        // the make it was won with is not.
        let stats = StatsStore::load(&root).await.unwrap();
        {
            let mut guard = stats.inner.lock().await;
            guard.players.insert(
                key(&AccountOwner::User(hero)),
                PlayerStats {
                    hands: 1,
                    hands_won: 1,
                    ..Default::default()
                },
            );
        }
        assert_eq!(stats.backfill_categories(&history).await.unwrap(), 1);
        let hero_stats = stats.of(&AccountOwner::User(hero)).await;
        assert_eq!(hero_stats.wins_shown, 1);
        assert_eq!(hero_stats.won_with(Category::StraightFlush), 1);
        // Everything else is left exactly as it was.
        assert_eq!(hero_stats.hands, 1);
        assert_eq!(hero_stats.hands_won, 1);

        // A second boot finds the watermark, so the count never doubles.
        assert_eq!(stats.backfill_categories(&history).await.unwrap(), 0);
        let reloaded = StatsStore::load(&root).await.unwrap();
        assert_eq!(reloaded.backfill_categories(&history).await.unwrap(), 0);
        assert_eq!(reloaded.of(&AccountOwner::User(hero)).await.wins_shown, 1);
    }
}
