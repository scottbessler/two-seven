use crate::{
    app::AppState,
    table::{SeatOccupant, TableMode},
};
use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::Serialize;
use std::{env, path::Path};

pub const PASSWORD_FILE: &str = "admin/password.txt";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ResetReport {
    pub accounts: usize,
    pub tables: usize,
    pub humans_kicked: usize,
}

pub async fn load_password(root: impl AsRef<Path>) -> Result<String> {
    load_password_from(root, env::var("ADMIN_PASSWORD").ok()).await
}

async fn load_password_from(
    root: impl AsRef<Path>,
    env_password: Option<String>,
) -> Result<String> {
    if let Some(password) = clean_password(env_password) {
        return Ok(password);
    }
    load_or_create_local_password(root).await
}

fn clean_password(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

async fn load_or_create_local_password(root: impl AsRef<Path>) -> Result<String> {
    let path = root.as_ref().join(PASSWORD_FILE);
    if let Ok(value) = tokio::fs::read_to_string(&path).await {
        let password = value.trim().to_string();
        if !password.is_empty() {
            return Ok(password);
        }
    }
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    let password = URL_SAFE_NO_PAD.encode(bytes);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    write_secret(&path, &password).await?;
    Ok(password)
}

#[cfg(unix)]
async fn write_secret(path: &Path, password: &str) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let path = path.to_path_buf();
    let password = format!("{password}\n");
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true).mode(0o600);
        std::io::Write::write_all(&mut options.open(path)?, password.as_bytes())?;
        Ok(())
    })
    .await??;
    Ok(())
}

#[cfg(not(unix))]
async fn write_secret(path: &Path, password: &str) -> Result<()> {
    tokio::fs::write(path, format!("{password}\n")).await?;
    Ok(())
}

pub async fn reset_money_and_loans(state: &AppState) -> Result<ResetReport> {
    let accounts = state.bank.reset_all().await?;
    let mut tables = 0;
    let mut humans_kicked = 0;
    for id in state.tables.ids().await {
        let Some(table) = state.tables.get(id).await else {
            continue;
        };
        let remove = {
            let table = table.lock().await;
            matches!(table.mode, TableMode::Tournament(_))
        };
        if remove {
            state.tables.remove(id).await?;
            tables += 1;
            continue;
        }
        state
            .tables
            .update(id, |table| {
                tables += 1;
                table.hand = None;
                table.last_hand = None;
                table.next_action_at = None;
                table.bot_hands_requested = 0;
                table.hand_no = 0;
                table.button = 0;
                for seat in &mut table.seats {
                    if matches!(seat.occupant, SeatOccupant::Human { .. }) {
                        humans_kicked += 1;
                    }
                    seat.occupant = SeatOccupant::Empty;
                    seat.stack = 0;
                    seat.sitting_out = false;
                    seat.pending_departure = false;
                }
                Ok(())
            })
            .await?;
    }
    crate::driver::ensure_cash_ladder(state).await?;
    Ok(ResetReport {
        accounts,
        tables,
        humans_kicked,
    })
}

pub fn password_matches(actual: &str, supplied: &str) -> bool {
    let actual = actual.as_bytes();
    let supplied = supplied.as_bytes();
    if actual.len() != supplied.len() {
        return false;
    }
    actual
        .iter()
        .zip(supplied)
        .fold(0u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn admin_password_prefers_the_environment() {
        let root = std::env::temp_dir().join(format!("two-seven-admin-env-{}", Uuid::new_v4()));
        let password = load_password_from(&root, Some("  from-env  ".into()))
            .await
            .unwrap();

        assert_eq!(password, "from-env");
        assert!(
            !root.join(PASSWORD_FILE).exists(),
            "an ADMIN_PASSWORD should not create a local fallback file"
        );
    }

    #[tokio::test]
    async fn admin_password_falls_back_to_a_local_secret_file() {
        let root = std::env::temp_dir().join(format!("two-seven-admin-file-{}", Uuid::new_v4()));
        let first = load_password_from(&root, None).await.unwrap();
        let second = load_password_from(&root, Some(" ".into())).await.unwrap();

        assert_eq!(first, second);
        assert_eq!(
            tokio::fs::read_to_string(root.join(PASSWORD_FILE))
                .await
                .unwrap()
                .trim(),
            first
        );
    }
}
