use crate::error::AppError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use uuid::Uuid;
use webauthn_rs::prelude::Passkey;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub credentials: Vec<Passkey>,
    #[serde(default)]
    pub settings: UserSettings,
    pub created_at: DateTime<Utc>,
}
/// Account-level options. They live on the server rather than in the browser
/// because the server is what enforces them: one relaxes a check on table
/// creation, the other decides what the table view is allowed to show.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSettings {
    /// Let this player run a tournament whose buy-in is more than they hold.
    /// They still have to afford the seat to register for it.
    #[serde(default)]
    pub unfunded_tournaments: bool,
    /// Show the bots' hole cards, but only while nobody else is seated: it is
    /// a practice aid, never something another player can be robbed by.
    #[serde(default)]
    pub see_bot_cards: bool,
}
pub fn normalize_username(x: &str) -> String {
    x.trim().to_lowercase()
}
struct Index {
    by_id: HashMap<Uuid, User>,
    by_username: HashMap<String, Uuid>,
}
pub struct UserStore {
    index: Mutex<Index>,
    dir: PathBuf,
}
impl UserStore {
    pub async fn load(root: impl Into<PathBuf>) -> Result<Self, AppError> {
        let dir = root.into().join("users");
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(AppError::internal)?;
        let (mut by_id, mut by_username) = (HashMap::new(), HashMap::new());
        let mut it = tokio::fs::read_dir(&dir)
            .await
            .map_err(AppError::internal)?;
        while let Some(e) = it.next_entry().await.map_err(AppError::internal)? {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            if let Ok(t) = tokio::fs::read_to_string(&p).await
                && let Ok(u) = serde_json::from_str::<User>(&t)
            {
                by_username.insert(normalize_username(&u.username), u.id);
                by_id.insert(u.id, u);
            }
        }
        Ok(Self {
            index: Mutex::new(Index { by_id, by_username }),
            dir,
        })
    }
    pub async fn get(&self, id: Uuid) -> Option<User> {
        self.index.lock().await.by_id.get(&id).cloned()
    }
    /// Everyone with an account, for the leaderboard.
    pub async fn all(&self) -> Vec<User> {
        self.index.lock().await.by_id.values().cloned().collect()
    }
    pub async fn get_by_username(&self, n: &str) -> Option<User> {
        let g = self.index.lock().await;
        g.by_username
            .get(&normalize_username(n))
            .and_then(|id| g.by_id.get(id))
            .cloned()
    }
    pub async fn username_taken(&self, n: &str) -> bool {
        self.index
            .lock()
            .await
            .by_username
            .contains_key(&normalize_username(n))
    }
    pub async fn insert(&self, u: User) -> Result<(), AppError> {
        let mut g = self.index.lock().await;
        let k = normalize_username(&u.username);
        if g.by_username.contains_key(&k) {
            return Err(AppError::conflict("that username is already taken"));
        }
        self.persist(&u).await?;
        g.by_username.insert(k, u.id);
        g.by_id.insert(u.id, u);
        Ok(())
    }
    /// Replaces one account's options. The stored user is the record of
    /// truth, so the write lands on disk before the index changes.
    pub async fn set_settings(&self, id: Uuid, settings: UserSettings) -> Result<(), AppError> {
        let mut g = self.index.lock().await;
        let mut updated = g
            .by_id
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::not_found("no such player"))?;
        updated.settings = settings;
        self.persist(&updated).await?;
        g.by_id.insert(id, updated);
        Ok(())
    }

    async fn persist(&self, u: &User) -> Result<(), AppError> {
        let p = self.dir.join(format!("{}.json", u.id));
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp = p.with_extension(format!("json.tmp-{n}"));
        tokio::fs::write(
            &tmp,
            serde_json::to_vec_pretty(u).map_err(AppError::internal)?,
        )
        .await
        .map_err(AppError::internal)?;
        tokio::fs::rename(tmp, p).await.map_err(AppError::internal)
    }
}
