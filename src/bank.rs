use crate::{
    money::{Cents, valid_game_amount},
    table::Bot,
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
    Bot(Bot),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum LedgerKind {
    ReUp,
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
    #[serde(default)]
    pub loan_count: u64,
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
    pub const RE_UP_AMOUNT: Cents = 100_000;
    pub const RE_UP_THRESHOLD: Cents = 10_000;

    pub async fn load(root: impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let dir = root.as_ref().join("bank");
        tokio::fs::create_dir_all(&dir).await?;
        let marker = dir.join("bank-v2-non-debt.marker");
        if !tokio::fs::try_exists(&marker).await? {
            let mut entries = tokio::fs::read_dir(&dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                if entry.path().extension().and_then(|x| x.to_str()) == Some("json") {
                    let _ = tokio::fs::remove_file(entry.path()).await;
                }
            }
            tokio::fs::write(&marker, b"legacy accounts wiped for non-debt bank\n").await?;
        }
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
                    loan_count: 0,
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
    /// Every account on the books, for the leaderboard.
    pub async fn accounts(&self) -> Vec<Account> {
        self.inner.lock().await.accounts.values().cloned().collect()
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
                    loan_count: 0,
                    entries: Vec::new(),
                    created_at: now,
                    updated_at: now,
                },
            );
        }
        let account = guard.accounts.get_mut(&owner).expect("account");
        if account.balance + delta < 0 {
            return Err(anyhow::anyhow!("insufficient funds"));
        }
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
        _no_debt: bool,
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
                    loan_count: 0,
                    entries: Vec::new(),
                    created_at: now,
                    updated_at: now,
                },
            );
        }
        if matches!(owner, AccountOwner::Bot(_)) {
            // The house covers a bot's seat in one loan. Lending a fixed
            // thousand at a time meant a $1,000,000 table cost a bot a
            // thousand loans and a thousand ledger lines per buy-in.
            let balance = guard.accounts[&owner].balance;
            if balance < amount {
                let shortfall = (amount - balance).max(Self::RE_UP_AMOUNT);
                Self::append_locked(
                    guard.accounts.get_mut(&owner).expect("account"),
                    LedgerKind::ReUp,
                    shortfall,
                    "re-up loan".into(),
                    true,
                );
            }
        }
        if amount > guard.accounts[&owner].balance {
            return Err(anyhow::anyhow!("insufficient funds"));
        }
        let account = guard.accounts.get_mut(&owner).expect("account");
        Self::append_locked(
            account,
            LedgerKind::BuyIn { table },
            -amount,
            "table buy-in".into(),
            false,
        );
        let result = account.clone();
        self.persist(&result).await?;
        Ok(result)
    }
    pub async fn re_up(&self, owner: AccountOwner) -> Result<Account, anyhow::Error> {
        let mut guard = self.inner.lock().await;
        if !guard.accounts.contains_key(&owner) {
            let now = Utc::now();
            guard.accounts.insert(
                owner.clone(),
                Account {
                    owner: owner.clone(),
                    balance: 0,
                    loan_count: 0,
                    entries: Vec::new(),
                    created_at: now,
                    updated_at: now,
                },
            );
        }
        if guard.accounts[&owner].balance >= Self::RE_UP_THRESHOLD {
            return Err(anyhow::anyhow!("re-up is only available below $100"));
        }
        let account = guard.accounts.get_mut(&owner).expect("account");
        Self::append_locked(
            account,
            LedgerKind::ReUp,
            Self::RE_UP_AMOUNT,
            "re-up loan".into(),
            true,
        );
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
            AccountOwner::Bot(bot) => format!("bot-{}-{}.json", bot.kind, bot.seat),
        };
        let path = self.dir.join(name);
        let tmp = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
        tokio::fs::write(&tmp, serde_json::to_vec_pretty(account)?).await?;
        tokio::fs::rename(tmp, path).await?;
        Ok(())
    }
    fn append_locked(
        account: &mut Account,
        kind: LedgerKind,
        delta: Cents,
        memo: String,
        loan: bool,
    ) {
        account.balance += delta;
        account.updated_at = Utc::now();
        if loan {
            account.loan_count += 1;
        }
        account.entries.push(LedgerEntry {
            id: Uuid::new_v4(),
            at: account.updated_at,
            kind,
            delta,
            balance_after: account.balance,
            memo,
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn a_bot_covers_a_big_seat_with_one_loan() {
        let root = std::env::temp_dir().join(format!("two-seven-bigseat-{}", Uuid::new_v4()));
        let bank = BankStore::load(&root).await.unwrap();
        let bot = AccountOwner::Bot(crate::table::Bot::new(crate::table::BotKind::Shark, 0));
        let table = Uuid::new_v4();
        // The dearest table costs a thousand times the standard loan.
        let account = bank
            .buy_in(bot.clone(), table, 100_000_000, false)
            .await
            .unwrap();
        assert_eq!(account.loan_count, 1, "one seat, one loan");
        assert_eq!(
            account.entries.len(),
            2,
            "a loan and a buy-in, not a thousand of each"
        );
        assert_eq!(account.balance, 0);
    }

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
    async fn game_entries_never_create_debt() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        assert!(
            bank.buy_in(owner.clone(), Uuid::new_v4(), 1_000_001, false)
                .await
                .is_err()
        );
        assert!(
            bank.buy_in(owner.clone(), Uuid::new_v4(), 100, false)
                .await
                .is_err()
        );
        bank.re_up(owner.clone()).await.unwrap();
        bank.buy_in(owner.clone(), Uuid::new_v4(), 100_000, false)
            .await
            .unwrap();
        assert_eq!(bank.account(owner).await.unwrap().balance, 0);
    }

    #[tokio::test]
    async fn re_up_requires_low_balance_and_tracks_shame() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        let account = bank.re_up(owner.clone()).await.unwrap();
        assert_eq!(account.balance, BankStore::RE_UP_AMOUNT);
        assert_eq!(account.loan_count, 1);
        assert!(bank.re_up(owner).await.is_err());
    }

    #[tokio::test]
    async fn bot_buy_ins_auto_re_up_without_debt() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::Bot(crate::table::Bot::new(crate::table::BotKind::Fish, 0));
        let account = bank
            .buy_in(owner, Uuid::new_v4(), 100, false)
            .await
            .unwrap();
        assert_eq!(account.balance, BankStore::RE_UP_AMOUNT - 100);
        assert_eq!(account.loan_count, 1);
    }

    #[tokio::test]
    async fn legacy_accounts_are_wiped_once() {
        let dir = tempfile_dir();
        let bank_dir = dir.join("bank");
        tokio::fs::create_dir_all(&bank_dir).await.unwrap();
        tokio::fs::write(
            bank_dir.join("user-00000000-0000-0000-0000-000000000000.json"),
            br#"{"legacy":true}"#,
        )
        .await
        .unwrap();
        let bank = BankStore::load(&dir).await.unwrap();
        let owner = AccountOwner::User(Uuid::nil());
        assert_eq!(bank.account(owner.clone()).await.unwrap().balance, 0);
        assert!(bank_dir.join("bank-v2-non-debt.marker").exists());
        drop(bank);
        let loaded = BankStore::load(&dir).await.unwrap();
        assert!(loaded.account(owner).await.unwrap().entries.is_empty());
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
    async fn buy_in_requires_balance_even_when_no_debt_flag_is_false() {
        let bank = BankStore::load(
            std::env::temp_dir().join(format!("two-seven-no-debt-{}", Uuid::new_v4())),
        )
        .await
        .unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        assert!(
            bank.buy_in(owner.clone(), Uuid::new_v4(), 1, false)
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
