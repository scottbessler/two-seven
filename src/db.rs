//! The SQLite handle every store will eventually share.
//!
//! The data directory keeps its shape — this database is one more file under
//! `DATA_PATH`, alongside the JSON trees it is replacing a store at a time.
//! Writes stay on local disk, which is what keeps a player's action off the
//! network; durability past the machine is Litestream's job, shipping the WAL
//! to object storage behind the process rather than in front of it.
//!
//! Every statement runs on the blocking pool behind one connection, so the
//! database sees the same single writer the JSON stores assumed. Readers
//! queue behind writers at this size, which costs nothing yet: the queries
//! this replaces read whole files. A read pool is the answer when it stops
//! being true, not before.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Statements the database is opened with. `NORMAL` is the durability WAL is
/// built for: a process crash loses nothing, and a machine crash loses only
/// the last checkpoint — strictly better than the write-then-rename the JSON
/// stores use today, which fsyncs neither the file nor the directory.
const PRAGMAS: &str = "
    PRAGMA journal_mode = WAL;
    PRAGMA synchronous = NORMAL;
    PRAGMA foreign_keys = ON;
    PRAGMA busy_timeout = 5000;
";

/// Ordered, append-only. A migration that has shipped is never edited; the
/// next change is the next entry.
const MIGRATIONS: &[&str] = &[
    // 1: hand history, one row per completed hand.
    "CREATE TABLE hands (
        table_id TEXT NOT NULL,
        hand_no  INTEGER NOT NULL,
        at       TEXT NOT NULL,
        record   TEXT NOT NULL,
        PRIMARY KEY (table_id, hand_no)
    ) WITHOUT ROWID;",
    // 2: what has already been carried over from the JSON tree, so an import
    // runs once and commits with the rows it moved rather than leaving a
    // marker file that can disagree with the data.
    "CREATE TABLE legacy_imports (
        name TEXT PRIMARY KEY,
        at   TEXT NOT NULL,
        rows INTEGER NOT NULL
    ) WITHOUT ROWID;",
];

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl Db {
    /// Open (creating if absent) the database under a `DATA_PATH` root.
    pub async fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        tokio::fs::create_dir_all(&root)
            .await
            .with_context(|| format!("failed to create data directory {}", root.display()))?;
        let path = root.join("two-seven.db");
        let opened = path.clone();
        let conn = tokio::task::spawn_blocking(move || -> Result<Connection> {
            let conn = Connection::open(&opened)
                .with_context(|| format!("failed to open {}", opened.display()))?;
            conn.execute_batch(PRAGMAS)
                .context("failed to configure the database")?;
            migrate(&conn)?;
            Ok(conn)
        })
        .await??;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            path,
        })
    }

    /// Where the database lives, for the operator-facing bits (backup,
    /// replication, the conservation checker).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Run one closure against the connection on the blocking pool.
    pub async fn call<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            // A poisoned lock means a previous caller panicked mid-statement.
            // The connection itself is still sound — SQLite rolls back an
            // unfinished transaction — so carry on rather than cascade.
            let mut guard = conn.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut guard)
        })
        .await?
    }
}

/// Apply every migration the database has not seen, in one transaction each.
/// `user_version` is the count applied so far.
fn migrate(conn: &Connection) -> Result<()> {
    let applied = conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))? as usize;
    if applied > MIGRATIONS.len() {
        anyhow::bail!(
            "database is at schema version {applied}, newer than this build's {}",
            MIGRATIONS.len()
        );
    }
    for (index, statement) in MIGRATIONS.iter().enumerate().skip(applied) {
        let version = index + 1;
        conn.execute_batch(&format!(
            "BEGIN; {statement} PRAGMA user_version = {version}; COMMIT;"
        ))
        .with_context(|| format!("migration {version} failed"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn scratch() -> (tempdir::Dir, Db) {
        let dir = tempdir::Dir::new();
        let db = Db::open(dir.path()).await.unwrap();
        (dir, db)
    }

    #[tokio::test]
    async fn migrations_apply_once_and_are_idempotent() {
        let (dir, db) = scratch().await;
        let version = db
            .call(|c| Ok(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?))
            .await
            .unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());
        drop(db);

        // Reopening the same file applies nothing and loses nothing.
        let db = Db::open(dir.path()).await.unwrap();
        let version = db
            .call(|c| Ok(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?))
            .await
            .unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());
    }

    #[tokio::test]
    async fn opens_in_wal_mode() {
        let (_dir, db) = scratch().await;
        let mode: String = db
            .call(|c| Ok(c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?))
            .await
            .unwrap();
        assert_eq!(mode, "wal");
    }

    /// A directory that cleans up after itself, so tests stop leaving trees
    /// behind in the system temp dir the way the JSON store tests do.
    pub mod tempdir {
        use std::path::{Path, PathBuf};
        pub struct Dir(PathBuf);
        impl Dir {
            pub fn new() -> Self {
                let path =
                    std::env::temp_dir().join(format!("two-seven-db-{}", uuid::Uuid::new_v4()));
                std::fs::create_dir_all(&path).unwrap();
                Self(path)
            }
            pub fn path(&self) -> &Path {
                &self.0
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}
