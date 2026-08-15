//! Per-table hand history.
//!
//! Every completed hand is appended to `history/<table id>.jsonl`, one JSON
//! record per line. The table document itself is rewritten on every action, so
//! history lives outside it: appending costs one write per hand instead of
//! growing the hot path, and the file stays greppable for debugging.

use crate::table::HandRecord;
use anyhow::Result;
use std::path::{Path, PathBuf};
use tokio::{fs::OpenOptions, io::AsyncWriteExt};
use uuid::Uuid;

/// How many hands a history view returns, newest last.
pub const HISTORY_PAGE: usize = 50;

#[derive(Clone)]
pub struct HistoryStore {
    dir: PathBuf,
}

impl HistoryStore {
    pub async fn load(root: impl AsRef<Path>) -> Result<Self> {
        let dir = root.as_ref().join("history");
        tokio::fs::create_dir_all(&dir).await?;
        Ok(Self { dir })
    }

    fn path(&self, table: Uuid) -> PathBuf {
        self.dir.join(format!("{table}.jsonl"))
    }

    pub async fn append(&self, table: Uuid, record: &HandRecord) -> Result<()> {
        let mut line = serde_json::to_vec(record)?;
        line.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path(table))
            .await?;
        file.write_all(&line).await?;
        file.flush().await?;
        Ok(())
    }

    /// The most recent hands, oldest first. A line that fails to parse is
    /// skipped rather than hiding every hand around it.
    pub async fn recent(&self, table: Uuid, limit: usize) -> Vec<HandRecord> {
        let Ok(text) = tokio::fs::read_to_string(self.path(table)).await else {
            return Vec::new();
        };
        let lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
        lines[lines.len().saturating_sub(limit)..]
            .iter()
            .filter_map(|line| serde_json::from_str::<HandRecord>(line).ok())
            .collect()
    }

    /// How many hands the table has on record.
    pub async fn count(&self, table: Uuid) -> usize {
        let Ok(text) = tokio::fs::read_to_string(self.path(table)).await else {
            return 0;
        };
        text.lines().filter(|line| !line.is_empty()).count()
    }
}
