use crate::{
    money::{Cents, valid_game_amount},
    table::BotKind,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum AccountOwner {
    User(Uuid),
    Bot(BotKind),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum LedgerKind {
    BuyIn { table: Uuid },
    CashOut { table: Uuid },
    TournamentBuyIn { tournament: Uuid },
    TournamentPrize { tournament: Uuid },
    HandBlitzBuyIn { run: Uuid },
    HandBlitzWin { run: Uuid },
    BlackjackBet { game: Uuid },
    BlackjackPayout { game: Uuid },
    Adjustment,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub id: Uuid,
    pub at: DateTime<Utc>,
    pub kind: LedgerKind,
    pub delta: Cents,
    pub balance_after: Cents,
    pub memo: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub owner: AccountOwner,
    pub balance: Cents,
    pub entries: Vec<LedgerEntry>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
struct Inner {
    accounts: HashMap<AccountOwner, Account>,
}
#[derive(Clone)]
pub struct BankStore {
    inner: Arc<Mutex<Inner>>,
    dir: PathBuf,
}
impl BankStore {
    pub async fn load(root: impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let dir = root.as_ref().join("bank");
        tokio::fs::create_dir_all(&dir).await?;
        let mut accounts = HashMap::new();
        let mut entries = tokio::fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.path().extension().and_then(|x| x.to_str()) == Some("json")
                && let Ok(data) = tokio::fs::read(entry.path()).await
                && let Ok(account) = serde_json::from_slice::<Account>(&data)
            {
                accounts.insert(account.owner.clone(), account);
            }
        }
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner { accounts })),
            dir,
        })
    }
    pub async fn account(&self, owner: AccountOwner) -> Result<Account, anyhow::Error> {
        let mut guard = self.inner.lock().await;
        if !guard.accounts.contains_key(&owner) {
            let now = Utc::now();
            guard.accounts.insert(
                owner.clone(),
                Account {
                    owner: owner.clone(),
                    balance: 0,
                    entries: Vec::new(),
                    created_at: now,
                    updated_at: now,
                },
            );
            self.persist(guard.accounts.get(&owner).expect("inserted"))
                .await?;
        }
        Ok(guard.accounts.get(&owner).expect("account").clone())
    }
    pub async fn append(
        &self,
        owner: AccountOwner,
        kind: LedgerKind,
        delta: Cents,
        memo: String,
    ) -> Result<Account, anyhow::Error> {
        let mut guard = self.inner.lock().await;
        if !guard.accounts.contains_key(&owner) {
            let now = Utc::now();
            guard.accounts.insert(
                owner.clone(),
                Account {
                    owner: owner.clone(),
                    balance: 0,
                    entries: Vec::new(),
                    created_at: now,
                    updated_at: now,
                },
            );
        }
        let account = guard.accounts.get_mut(&owner).expect("account");
        account.balance += delta;
        account.updated_at = Utc::now();
        account.entries.push(LedgerEntry {
            id: Uuid::new_v4(),
            at: account.updated_at,
            kind,
            delta,
            balance_after: account.balance,
            memo,
        });
        let result = account.clone();
        self.persist(&result).await?;
        Ok(result)
    }
    pub async fn buy_in(
        &self,
        owner: AccountOwner,
        table: Uuid,
        amount: Cents,
        no_debt: bool,
    ) -> Result<Account, anyhow::Error> {
        if !valid_game_amount(amount) {
            return Err(anyhow::anyhow!("game entry must be between $1 and $10,000"));
        }
        let mut guard = self.inner.lock().await;
        if !guard.accounts.contains_key(&owner) {
            let now = Utc::now();
            guard.accounts.insert(
                owner.clone(),
                Account {
                    owner: owner.clone(),
                    balance: 0,
                    entries: Vec::new(),
                    created_at: now,
                    updated_at: now,
                },
            );
        }
        if no_debt
            && (guard.accounts[&owner].balance <= 0 || amount > guard.accounts[&owner].balance)
        {
            return Err(anyhow::anyhow!("insufficient funds for no-debt table"));
        }
        let account = guard.accounts.get_mut(&owner).expect("account");
        account.balance -= amount;
        account.updated_at = Utc::now();
        account.entries.push(LedgerEntry {
            id: Uuid::new_v4(),
            at: account.updated_at,
            kind: LedgerKind::BuyIn { table },
            delta: -amount,
            balance_after: account.balance,
            memo: "table buy-in".into(),
        });
        let result = account.clone();
        self.persist(&result).await?;
        Ok(result)
    }
    pub async fn cash_out(
        &self,
        owner: AccountOwner,
        table: Uuid,
        amount: Cents,
    ) -> Result<Account, anyhow::Error> {
        self.append(
            owner,
            LedgerKind::CashOut { table },
            amount,
            "table cash-out".into(),
        )
        .await
    }
    pub async fn hand_blitz_buy_in(
        &self,
        owner: AccountOwner,
        run: Uuid,
        amount: Cents,
    ) -> Result<Account, anyhow::Error> {
        if !valid_game_amount(amount) {
            return Err(anyhow::anyhow!("game entry must be between $1 and $10,000"));
        }
        self.append(
            owner,
            LedgerKind::HandBlitzBuyIn { run },
            -amount,
            "hand blitz buy-in".into(),
        )
        .await
    }
    pub async fn hand_blitz_win(
        &self,
        owner: AccountOwner,
        run: Uuid,
        amount: Cents,
    ) -> Result<Account, anyhow::Error> {
        if amount <= 0 {
            return Err(anyhow::anyhow!("win amount must be positive"));
        }
        self.append(
            owner,
            LedgerKind::HandBlitzWin { run },
            amount,
            "hand blitz win".into(),
        )
        .await
    }
    pub async fn blackjack_bet(
        &self,
        owner: AccountOwner,
        game: Uuid,
        amount: Cents,
    ) -> Result<Account, anyhow::Error> {
        if !valid_game_amount(amount) {
            return Err(anyhow::anyhow!("game entry must be between $1 and $10,000"));
        }
        self.append(
            owner,
            LedgerKind::BlackjackBet { game },
            -amount,
            "blackjack bet".into(),
        )
        .await
    }
    pub async fn blackjack_payout(
        &self,
        owner: AccountOwner,
        game: Uuid,
        amount: Cents,
    ) -> Result<Account, anyhow::Error> {
        if amount <= 0 {
            return Err(anyhow::anyhow!("payout amount must be positive"));
        }
        self.append(
            owner,
            LedgerKind::BlackjackPayout { game },
            amount,
            "blackjack payout".into(),
        )
        .await
    }
    async fn persist(&self, account: &Account) -> Result<(), anyhow::Error> {
        let name = match account.owner {
            AccountOwner::User(id) => format!("user-{id}.json"),
            AccountOwner::Bot(kind) => format!("bot-{kind}.json"),
        };
        let path = self.dir.join(name);
        let tmp = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
        tokio::fs::write(&tmp, serde_json::to_vec_pretty(account)?).await?;
        tokio::fs::rename(tmp, path).await?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn ledger_balances_match() {
        let dir = tempfile_dir();
        let bank = BankStore::load(&dir).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        bank.append(owner.clone(), LedgerKind::Adjustment, 100, "seed".into())
            .await
            .unwrap();
        let a = bank
            .append(owner, LedgerKind::Adjustment, -25, "spend".into())
            .await
            .unwrap();
        assert_eq!(a.balance, 75);
        assert_eq!(a.entries.iter().map(|e| e.delta).sum::<i64>(), a.balance);
        assert!(
            a.entries
                .iter()
                .enumerate()
                .all(|(i, e)| e.balance_after
                    == a.entries[..=i].iter().map(|x| x.delta).sum::<i64>())
        );
    }

    #[tokio::test]
    async fn game_entry_cap_is_per_transaction_not_cumulative_debt() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        assert!(
            bank.buy_in(owner.clone(), Uuid::new_v4(), 1_000_001, false)
                .await
                .is_err()
        );
        bank.buy_in(owner.clone(), Uuid::new_v4(), 1_000_000, false)
            .await
            .unwrap();
        bank.buy_in(owner.clone(), Uuid::new_v4(), 1_000_000, false)
            .await
            .unwrap();
        assert_eq!(bank.account(owner).await.unwrap().balance, -2_000_000);
    }

    #[tokio::test]
    async fn game_payouts_require_positive_amounts() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        assert!(
            bank.hand_blitz_win(owner.clone(), Uuid::new_v4(), 0)
                .await
                .is_err()
        );
        assert!(
            bank.hand_blitz_win(owner.clone(), Uuid::new_v4(), -1)
                .await
                .is_err()
        );
        assert!(
            bank.blackjack_payout(owner.clone(), Uuid::new_v4(), 0)
                .await
                .is_err()
        );
        assert!(
            bank.blackjack_payout(owner, Uuid::new_v4(), -1)
                .await
                .is_err()
        );
    }

    fn tempfile_dir() -> PathBuf {
        std::env::temp_dir().join(format!("two-seven-bank-{}", Uuid::new_v4()))
    }
}

#[cfg(test)]
mod no_debt_tests {
    use super::*;
    #[tokio::test]
    async fn no_debt_requires_positive_balance_and_covers_buy_in() {
        let bank = BankStore::load(
            std::env::temp_dir().join(format!("two-seven-no-debt-{}", Uuid::new_v4())),
        )
        .await
        .unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        assert!(
            bank.buy_in(owner.clone(), Uuid::new_v4(), 1, true)
                .await
                .is_err()
        );
        bank.append(owner.clone(), LedgerKind::Adjustment, 100, "seed".into())
            .await
            .unwrap();
        assert!(
            bank.buy_in(owner.clone(), Uuid::new_v4(), 101, true)
                .await
                .is_err()
        );
        assert_eq!(
            bank.buy_in(owner, Uuid::new_v4(), 100, true)
                .await
                .unwrap()
                .balance,
            0
        );
    }
}
