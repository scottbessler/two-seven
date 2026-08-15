use crate::table::Table;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{Mutex, broadcast};
use uuid::Uuid;
#[derive(Clone)]
pub struct TableStore {
    tables: Arc<Mutex<HashMap<Uuid, Arc<Mutex<Table>>>>>,
    dir: PathBuf,
    changed: broadcast::Sender<Uuid>,
}
impl TableStore {
    pub async fn load(root: impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let dir = root.as_ref().join("tables");
        tokio::fs::create_dir_all(&dir).await?;
        let (mut map, entries) = (HashMap::new(), tokio::fs::read_dir(&dir).await?);
        let (tx, _) = broadcast::channel(128);
        let mut entries = entries;
        while let Some(e) = entries.next_entry().await? {
            if let Ok(bytes) = tokio::fs::read(e.path()).await
                && let Ok(table) = serde_json::from_slice::<Table>(&bytes)
            {
                map.insert(table.id, Arc::new(Mutex::new(table)));
            }
        }
        Ok(Self {
            tables: Arc::new(Mutex::new(map)),
            dir,
            changed: tx,
        })
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Uuid> {
        self.changed.subscribe()
    }
    pub async fn get(&self, id: Uuid) -> Option<Arc<Mutex<Table>>> {
        self.tables.lock().await.get(&id).cloned()
    }
    pub async fn ids(&self) -> Vec<Uuid> {
        self.tables.lock().await.keys().copied().collect()
    }
    pub async fn insert(&self, table: Table) -> Result<Uuid, anyhow::Error> {
        let id = table.id;
        self.persist(&table).await?;
        self.tables
            .lock()
            .await
            .insert(id, Arc::new(Mutex::new(table)));
        let _ = self.changed.send(id);
        Ok(id)
    }
    /// Forget a table entirely, file and all.
    pub async fn remove(&self, id: Uuid) -> Result<(), anyhow::Error> {
        self.tables.lock().await.remove(&id);
        let path = self.dir.join(format!("{id}.json"));
        if tokio::fs::try_exists(&path).await? {
            tokio::fs::remove_file(path).await?;
        }
        let _ = self.changed.send(id);
        Ok(())
    }
    pub async fn update<F>(&self, id: Uuid, f: F) -> Result<(), anyhow::Error>
    where
        F: FnOnce(&mut Table) -> Result<(), anyhow::Error>,
    {
        let arc = self
            .get(id)
            .await
            .ok_or_else(|| anyhow::anyhow!("table not found"))?;
        let mut table = arc.lock().await;
        f(&mut table)?;
        table.updated_at = chrono::Utc::now();
        self.persist(&table).await?;
        let _ = self.changed.send(id);
        Ok(())
    }
    async fn persist(&self, table: &Table) -> Result<(), anyhow::Error> {
        let path = self.dir.join(format!("{}.json", table.id));
        let tmp = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
        tokio::fs::write(&tmp, serde_json::to_vec_pretty(table)?).await?;
        tokio::fs::rename(tmp, path).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{Stakes, Table, TableMode};
    #[tokio::test]
    async fn update_persists_and_broadcasts() {
        let root = std::env::temp_dir().join(format!("two-seven-store-{}", Uuid::new_v4()));
        let store = TableStore::load(&root).await.unwrap();
        let table = Table::new(
            "test".into(),
            Stakes::NoLimit {
                small_blind: 1,
                big_blind: 2,
            },
            TableMode::Cash { no_debt: false },
            2,
            100,
        );
        let id = table.id;
        store.insert(table).await.unwrap();
        let mut events = store.subscribe();
        store
            .update(id, |table| {
                table.name = "changed".into();
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(events.recv().await.unwrap(), id);
        assert_eq!(store.get(id).await.unwrap().lock().await.name, "changed");
    }
}
