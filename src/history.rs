//! Per-table hand history.
//!
//! Every completed hand is a row in `hands`, keyed by table and hand number.
//! The record itself stays a JSON blob: it is always read whole, and leaving
//! it serialized means `HandRecord` can keep changing shape without a
//! migration. What SQL buys here is the reading — the JSONL file this
//! replaces had to be parsed end to end to answer either question a history
//! page asks, so a table's hundredth hand cost a hundred hands of work and
//! its thousandth cost a thousand. Both are now indexed lookups.

use crate::{db::Db, table::HandRecord};
use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};
use std::path::Path;
use uuid::Uuid;

/// How many hands a history view returns, newest last.
pub const HISTORY_PAGE: usize = 50;

#[derive(Clone)]
pub struct HistoryStore {
    db: Db,
}

impl HistoryStore {
    /// Open the database under a `DATA_PATH` root and carry over whatever the
    /// JSONL tree still holds. Tests and `cargo run` on a fresh directory get
    /// an empty database; a deployed machine gets its hands moved across on
    /// the first boot after this ships.
    pub async fn load(root: impl AsRef<Path>) -> Result<Self> {
        let db = Db::open(&root).await?;
        let store = Self::new(db);
        store.import_legacy_jsonl(root).await?;
        Ok(store)
    }

    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub async fn append(&self, table: Uuid, record: &HandRecord) -> Result<()> {
        let (table, hand_no) = (table.to_string(), record.hand_no as i64);
        let at = record.at.to_rfc3339();
        let json = serde_json::to_string(record)?;
        self.db
            .call(move |conn| {
                // A hand number is replayed only when a settle is retried, in
                // which case the later record is the true one.
                conn.execute(
                    "INSERT INTO hands (table_id, hand_no, at, record) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (table_id, hand_no) DO UPDATE SET at = ?3, record = ?4",
                    params![table, hand_no, at, json],
                )?;
                Ok(())
            })
            .await
            .context("failed to record a hand")
    }

    /// The most recent hands, oldest first. A row that fails to parse is
    /// skipped rather than hiding every hand around it.
    pub async fn recent(&self, table: Uuid, limit: usize) -> Vec<HandRecord> {
        let table = table.to_string();
        let rows = self
            .db
            .call(move |conn| {
                let mut statement = conn.prepare_cached(
                    "SELECT record FROM hands WHERE table_id = ?1
                     ORDER BY hand_no DESC LIMIT ?2",
                )?;
                let rows = statement
                    .query_map(params![table, limit as i64], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await;
        let mut hands: Vec<HandRecord> = match rows {
            Ok(rows) => rows
                .iter()
                .filter_map(|json| serde_json::from_str(json).ok())
                .collect(),
            Err(error) => {
                tracing::error!(%error, "failed to read hand history");
                return Vec::new();
            }
        };
        hands.reverse();
        hands
    }

    /// Every hand every table has ever played, for a one-off walk over the
    /// whole record. Read one file at a time rather than all at once: the
    /// history outgrows the tables themselves, and only the backfill needs it.
    pub async fn every_hand(&self) -> Vec<HandRecord> {
        let mut hands = Vec::new();
        let Ok(mut entries) = tokio::fs::read_dir(&self.dir).await else {
            return hands;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.path().extension().is_none_or(|ext| ext != "jsonl") {
                continue;
            }
            let Ok(text) = tokio::fs::read_to_string(entry.path()).await else {
                continue;
            };
            hands.extend(
                text.lines()
                    .filter(|line| !line.is_empty())
                    .filter_map(|line| serde_json::from_str::<HandRecord>(line).ok()),
            );
        }
        hands.sort_by_key(|hand| hand.at);
        hands
    }

    /// How many hands the table has on record.
    pub async fn count(&self, table: Uuid) -> usize {
        let table = table.to_string();
        self.db
            .call(move |conn| {
                let count: i64 = conn.query_row(
                    "SELECT count(*) FROM hands WHERE table_id = ?1",
                    params![table],
                    |row| row.get(0),
                )?;
                Ok(count as usize)
            })
            .await
            .unwrap_or_else(|error| {
                tracing::error!(%error, "failed to count hand history");
                0
            })
    }

    /// Move `history/<table>.jsonl` into the database, once. The import is
    /// recorded in the same transaction as the rows it inserts, so a crash
    /// halfway through leaves the JSONL tree authoritative and the next boot
    /// starts over. The files are left in place: they are the fallback until
    /// the deployed database has proven itself.
    async fn import_legacy_jsonl(&self, root: impl AsRef<Path>) -> Result<()> {
        const IMPORT: &str = "history/jsonl";
        if self.already_imported(IMPORT).await? {
            return Ok(());
        }
        let dir = root.as_ref().join("history");
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            // No legacy tree at all — a fresh install. Record it so we do not
            // go looking again on every boot.
            return self.record_import(IMPORT, 0).await;
        };
        let mut hands = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(table) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| Uuid::parse_str(stem).ok())
            else {
                tracing::warn!(path = %path.display(), "skipping unrecognised history file");
                continue;
            };
            let Ok(text) = tokio::fs::read_to_string(&path).await else {
                continue;
            };
            for line in text.lines().filter(|line| !line.is_empty()) {
                match serde_json::from_str::<HandRecord>(line) {
                    Ok(record) => hands.push((table, record)),
                    Err(error) => {
                        tracing::warn!(path = %path.display(), %error, "skipping unreadable hand")
                    }
                }
            }
        }
        let imported = hands.len();
        self.db
            .call(move |conn| {
                let transaction = conn.transaction()?;
                {
                    let mut insert = transaction.prepare(
                        "INSERT OR REPLACE INTO hands (table_id, hand_no, at, record)
                         VALUES (?1, ?2, ?3, ?4)",
                    )?;
                    for (table, record) in &hands {
                        insert.execute(params![
                            table.to_string(),
                            record.hand_no as i64,
                            record.at.to_rfc3339(),
                            serde_json::to_string(record)?,
                        ])?;
                    }
                }
                transaction.execute(
                    "INSERT INTO legacy_imports (name, at, rows) VALUES (?1, ?2, ?3)",
                    params![IMPORT, chrono::Utc::now().to_rfc3339(), imported as i64],
                )?;
                transaction.commit()?;
                Ok(())
            })
            .await
            .context("failed to import legacy hand history")?;
        if imported > 0 {
            tracing::info!(hands = imported, "imported hand history from JSONL");
        }
        Ok(())
    }

    async fn already_imported(&self, name: &'static str) -> Result<bool> {
        self.db
            .call(move |conn| {
                let seen: Option<i64> = conn
                    .query_row(
                        "SELECT rows FROM legacy_imports WHERE name = ?1",
                        params![name],
                        |row| row.get(0),
                    )
                    .optional()?;
                Ok(seen.is_some())
            })
            .await
    }

    async fn record_import(&self, name: &'static str, rows: usize) -> Result<()> {
        self.db
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO legacy_imports (name, at, rows) VALUES (?1, ?2, ?3)",
                    params![name, chrono::Utc::now().to_rfc3339(), rows as i64],
                )?;
                Ok(())
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Stakes;

    struct Dir(std::path::PathBuf);
    impl Dir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("two-seven-history-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn hand(table: Uuid, hand_no: u64) -> HandRecord {
        HandRecord {
            table,
            hand_no,
            at: chrono::Utc::now(),
            stakes: Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            button: 0,
            seats: Vec::new(),
            summary: serde_json::from_value(serde_json::json!({
                "board": [],
                "results": [],
                "awards": [],
                "contributions": {},
                "revealed_hole_cards": [],
            }))
            .unwrap(),
        }
    }

    #[tokio::test]
    async fn recent_returns_the_last_hands_oldest_first() {
        let dir = Dir::new();
        let store = HistoryStore::load(&dir.0).await.unwrap();
        let table = Uuid::new_v4();
        for hand_no in 1..=5 {
            store.append(table, &hand(table, hand_no)).await.unwrap();
        }
        assert_eq!(store.count(table).await, 5);
        let recent = store.recent(table, 3).await;
        assert_eq!(
            recent.iter().map(|h| h.hand_no).collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
    }

    #[tokio::test]
    async fn hands_are_kept_apart_by_table() {
        let dir = Dir::new();
        let store = HistoryStore::load(&dir.0).await.unwrap();
        let (one, two) = (Uuid::new_v4(), Uuid::new_v4());
        store.append(one, &hand(one, 1)).await.unwrap();
        store.append(two, &hand(two, 1)).await.unwrap();
        store.append(two, &hand(two, 2)).await.unwrap();
        assert_eq!(store.count(one).await, 1);
        assert_eq!(store.count(two).await, 2);
        assert!(store.recent(Uuid::new_v4(), 10).await.is_empty());
    }

    #[tokio::test]
    async fn a_replayed_hand_number_overwrites_rather_than_duplicating() {
        let dir = Dir::new();
        let store = HistoryStore::load(&dir.0).await.unwrap();
        let table = Uuid::new_v4();
        let mut record = hand(table, 7);
        store.append(table, &record).await.unwrap();
        record.button = 3;
        store.append(table, &record).await.unwrap();
        assert_eq!(store.count(table).await, 1);
        assert_eq!(store.recent(table, 10).await[0].button, 3);
    }

    #[tokio::test]
    async fn legacy_jsonl_is_imported_once() {
        let dir = Dir::new();
        let table = Uuid::new_v4();
        let history = dir.0.join("history");
        std::fs::create_dir_all(&history).unwrap();
        let mut lines = String::new();
        for hand_no in 1..=3 {
            lines.push_str(&serde_json::to_string(&hand(table, hand_no)).unwrap());
            lines.push('\n');
        }
        // A torn last line is what an unclean shutdown leaves behind; it must
        // not take the hands before it down with it.
        lines.push_str("{\"table\":\"not json\"\n");
        std::fs::write(history.join(format!("{table}.jsonl")), lines).unwrap();
        std::fs::write(history.join("notes.txt"), "ignored").unwrap();

        let store = HistoryStore::load(&dir.0).await.unwrap();
        assert_eq!(store.count(table).await, 3);

        // Reopening does not import a second time, and does not resurrect a
        // hand the database has since been told to forget.
        store
            .db
            .call(move |conn| {
                conn.execute("DELETE FROM hands WHERE hand_no = 1", [])?;
                Ok(())
            })
            .await
            .unwrap();
        let store = HistoryStore::load(&dir.0).await.unwrap();
        assert_eq!(store.count(table).await, 2);
    }

    #[tokio::test]
    async fn a_fresh_install_with_no_legacy_tree_loads_clean() {
        let dir = Dir::new();
        let store = HistoryStore::load(&dir.0).await.unwrap();
        assert_eq!(store.count(Uuid::new_v4()).await, 0);
    }
}
