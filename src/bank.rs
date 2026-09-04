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
    HouseStake,
    LoanRepayment,
    LoanInterest,
    BuyIn { table: Uuid },
    CashOut { table: Uuid },
    TournamentBuyIn { tournament: Uuid },
    TournamentPrize { tournament: Uuid },
    HandBlitzBuyIn { run: Uuid },
    HandBlitzWin { run: Uuid },
    BlackjackBet { game: Uuid },
    BlackjackPayout { game: Uuid },
    BlackjackBuyIn { table: Uuid },
    BlackjackCashOut { table: Uuid },
    Gift { peer: AccountOwner },
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
impl Account {
    pub fn loan_debt(&self) -> Cents {
        self.loan_count as Cents * BankStore::RE_UP_AMOUNT
    }
    pub fn net_balance(&self) -> Cents {
        self.balance - self.loan_debt()
    }
    pub fn next_loan_repayment_amount(&self) -> Option<Cents> {
        (self.loan_count > 0).then_some(BankStore::RE_UP_AMOUNT)
    }
    /// How many whole loans the balance could pay off right now.
    pub fn repayable_loans(&self) -> u64 {
        self.loan_count
            .min((self.balance / BankStore::RE_UP_AMOUNT).max(0) as u64)
    }
    /// Every account this one has traded gifts with, the person given the
    /// most to first.
    /// Gifts are the only entries that name a counterparty, so this is the
    /// whole of who owes whom a thank-you.
    pub fn gift_tallies(&self) -> Vec<GiftTally> {
        let mut tallies: Vec<GiftTally> = Vec::new();
        for entry in &self.entries {
            let LedgerKind::Gift { peer } = &entry.kind else {
                continue;
            };
            let tally = match tallies.iter_mut().find(|tally| &tally.peer == peer) {
                Some(tally) => tally,
                None => {
                    tallies.push(GiftTally {
                        peer: peer.clone(),
                        received: 0,
                        sent: 0,
                    });
                    tallies.last_mut().expect("just pushed")
                }
            };
            if entry.delta >= 0 {
                tally.received += entry.delta;
            } else {
                tally.sent += -entry.delta;
            }
        }
        tallies.sort_by_key(|tally| std::cmp::Reverse(tally.net()));
        tallies
    }
}
/// One counterparty's side of the gift ledger: what they handed over, what
/// they were handed, and so which way the two accounts stand. Generosity is
/// the positive direction: the net is what you are out of pocket to them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GiftTally {
    pub peer: AccountOwner,
    pub received: Cents,
    pub sent: Cents,
}
impl GiftTally {
    pub fn net(&self) -> Cents {
        self.sent - self.received
    }
}

pub fn account_json(account: &Account) -> serde_json::Value {
    let mut value = serde_json::to_value(account).expect("account serializes");
    let object = value.as_object_mut().expect("account is an object");
    object.insert("loan_debt".into(), account.loan_debt().into());
    object.insert("net_balance".into(), account.net_balance().into());
    object.insert(
        "can_re_up".into(),
        (account.balance < BankStore::RE_UP_THRESHOLD).into(),
    );
    object.insert(
        "next_repayment_amount".into(),
        account.next_loan_repayment_amount().into(),
    );
    object.insert("repayable_loans".into(), account.repayable_loans().into());
    value
}
/// A gift is a positive, whole number of $1,000 chips, bounded by the largest
/// amount that may ride on anything else in the game.
pub fn valid_gift_amount(amount: Cents) -> bool {
    (BankStore::GIFT_INCREMENT..=BankStore::MAX_GIFT).contains(&amount)
        && amount % BankStore::GIFT_INCREMENT == 0
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
    pub const RE_UP_THRESHOLD: Cents = Self::RE_UP_AMOUNT;
    /// The dearest seat a person may borrow their way into. Lending covers the
    /// bottom of the ladder, where a broke player would otherwise have nothing
    /// to play; the deeper games are for money you already have. The house is
    /// not held to it: its regulars are staked at every rung (§V10).
    pub const LOAN_BUY_IN_LIMIT: Cents = Self::RE_UP_AMOUNT;
    /// Money changes hands between people in whole $1,000 chips, the same unit
    /// a loan comes in, capped at one gift per the largest game entry.
    pub const GIFT_INCREMENT: Cents = 100_000;
    pub const MAX_GIFT: Cents = crate::money::MAX_GAME_ENTRY;
    /// Bump this to start the house over on the next boot: money, loans,
    /// history and their playing record.
    pub const HOUSE_RESET_MARKER: &'static str = "bank-house-reset-2.marker";

    pub async fn load(root: impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        Ok(Self::load_reporting_reset(root).await?.0)
    }

    /// Loads, and says whether this boot wiped the house's accounts, so their
    /// playing record can be cleared with them.
    pub async fn load_reporting_reset(
        root: impl AsRef<Path>,
    ) -> Result<(Self, bool), anyhow::Error> {
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
        // The house's books start over: no balance, no loans, no history. Bump
        // HOUSE_RESET_MARKER to wipe them again on a later release; people's
        // accounts are never touched.
        let mut reset_house = false;
        let bots_marker = dir.join(Self::HOUSE_RESET_MARKER);
        if !tokio::fs::try_exists(&bots_marker).await? {
            let mut entries = tokio::fs::read_dir(&dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.extension().and_then(|x| x.to_str()) == Some("json")
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("bot-"))
                {
                    let _ = tokio::fs::remove_file(path).await;
                }
            }
            tokio::fs::write(&bots_marker, b"house accounts reset\n").await?;
            reset_house = true;
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
        Ok((
            Self {
                inner: Arc::new(Mutex::new(Inner { accounts })),
                dir,
            },
            reset_house,
        ))
    }
    pub async fn account(&self, owner: AccountOwner) -> Result<Account, anyhow::Error> {
        let mut guard = self.inner.lock().await;
        if !guard.accounts.contains_key(&owner) {
            Self::ensure_account_locked(&mut guard.accounts, &owner);
            self.persist(guard.accounts.get(&owner).expect("inserted"))
                .await?;
        }
        Ok(guard.accounts.get(&owner).expect("account").clone())
    }
    /// Every account on the books, for the leaderboard.
    pub async fn accounts(&self) -> Vec<Account> {
        self.inner.lock().await.accounts.values().cloned().collect()
    }

    pub async fn forgive_bot_loans(&self) -> Result<BotLoanForgivenessReport, anyhow::Error> {
        let mut guard = self.inner.lock().await;
        let mut changed = Vec::new();
        let mut loans = 0;
        for account in guard.accounts.values_mut() {
            if matches!(account.owner, AccountOwner::Bot(_)) && account.loan_count > 0 {
                loans += account.loan_count;
                account.loan_count = 0;
                account.updated_at = Utc::now();
                changed.push(account.clone());
            }
        }
        for account in &changed {
            self.persist(account).await?;
        }
        Ok(BotLoanForgivenessReport {
            accounts: changed.len(),
            loans,
        })
    }

    pub async fn reset_all(&self) -> Result<usize, anyhow::Error> {
        let removed = {
            let mut guard = self.inner.lock().await;
            let removed = guard.accounts.len();
            guard.accounts.clear();
            removed
        };
        let mut entries = tokio::fs::read_dir(&self.dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.path().extension().and_then(|x| x.to_str()) == Some("json") {
                tokio::fs::remove_file(entry.path()).await?;
            }
        }
        Ok(removed)
    }

    pub async fn append(
        &self,
        owner: AccountOwner,
        kind: LedgerKind,
        delta: Cents,
        memo: String,
    ) -> Result<Account, anyhow::Error> {
        let mut guard = self.inner.lock().await;
        Self::ensure_account_locked(&mut guard.accounts, &owner);
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
        no_debt: bool,
    ) -> Result<Account, anyhow::Error> {
        if !valid_game_amount(amount) {
            return Err(anyhow::anyhow!("game entry must be between $1 and $10,000"));
        }
        let mut guard = self.inner.lock().await;
        Self::ensure_account_locked(&mut guard.accounts, &owner);
        let lends = matches!(owner, AccountOwner::Bot(_))
            || (!no_debt && amount <= Self::LOAN_BUY_IN_LIMIT);
        if lends {
            let balance = guard.accounts[&owner].balance;
            if balance < amount {
                let shortfall = amount - balance;
                let loans = ((shortfall + Self::RE_UP_AMOUNT - 1) / Self::RE_UP_AMOUNT) as u64;
                let loan_amount = loans as Cents * Self::RE_UP_AMOUNT;
                let first_bot_funding = matches!(&owner, AccountOwner::Bot(_))
                    && guard.accounts[&owner].entries.iter().all(|entry| {
                        !matches!(&entry.kind, LedgerKind::ReUp | LedgerKind::HouseStake)
                    });
                Self::append_locked(
                    guard.accounts.get_mut(&owner).expect("account"),
                    if first_bot_funding {
                        LedgerKind::HouseStake
                    } else {
                        LedgerKind::ReUp
                    },
                    loan_amount,
                    if first_bot_funding {
                        "house stake".into()
                    } else if loans == 1 {
                        "re-up loan".into()
                    } else {
                        format!("re-up loan ({loans} loans)")
                    },
                    if first_bot_funding { 0 } else { loans },
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
            0,
        );
        let result = account.clone();
        self.persist(&result).await?;
        Ok(result)
    }
    pub async fn re_up(&self, owner: AccountOwner) -> Result<Account, anyhow::Error> {
        let mut guard = self.inner.lock().await;
        Self::ensure_account_locked(&mut guard.accounts, &owner);
        if guard.accounts[&owner].balance >= Self::RE_UP_THRESHOLD {
            return Err(anyhow::anyhow!("re-up is only available below $1,000"));
        }
        let account = guard.accounts.get_mut(&owner).expect("account");
        Self::append_locked(
            account,
            LedgerKind::ReUp,
            Self::RE_UP_AMOUNT,
            "re-up loan".into(),
            1,
        );
        let result = account.clone();
        self.persist(&result).await?;
        Ok(result)
    }
    pub async fn repay_loan(&self, owner: AccountOwner) -> Result<Account, anyhow::Error> {
        let mut guard = self.inner.lock().await;
        Self::ensure_account_locked(&mut guard.accounts, &owner);
        let account = guard.accounts.get_mut(&owner).expect("account");
        if account.loan_count == 0 {
            return Err(anyhow::anyhow!("no outstanding loans"));
        }
        if account.balance < Self::RE_UP_AMOUNT {
            return Err(anyhow::anyhow!("not enough to pay back that loan"));
        }
        account.loan_count -= 1;
        Self::append_locked(
            account,
            LedgerKind::LoanRepayment,
            -Self::RE_UP_AMOUNT,
            "loan repayment".into(),
            0,
        );
        let result = account.clone();
        self.persist(&result).await?;
        Ok(result)
    }
    /// Clear as much debt as the balance covers, in one go. Repaying loan by
    /// loan is the same money; this is the button for someone who has just had
    /// a good night and wants to be square.
    pub async fn repay_all_loans(&self, owner: AccountOwner) -> Result<Account, anyhow::Error> {
        let mut guard = self.inner.lock().await;
        Self::ensure_account_locked(&mut guard.accounts, &owner);
        let account = guard.accounts.get_mut(&owner).expect("account");
        if account.loan_count == 0 {
            return Err(anyhow::anyhow!("no outstanding loans"));
        }
        let loans = account.repayable_loans();
        if loans == 0 {
            return Err(anyhow::anyhow!("not enough to pay back that loan"));
        }
        account.loan_count -= loans;
        Self::append_locked(
            account,
            LedgerKind::LoanRepayment,
            -(loans as Cents * Self::RE_UP_AMOUNT),
            if loans == 1 {
                "loan repayment".into()
            } else {
                format!("loan repayment ({loans} loans)")
            },
            0,
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
        let mut guard = self.inner.lock().await;
        Self::ensure_account_locked(&mut guard.accounts, &owner);
        let account = guard.accounts.get_mut(&owner).expect("account");
        let loan_count = account.loan_count.min(10);
        let staked = account
            .entries
            .iter()
            .fold(0, |total, entry| match &entry.kind {
                LedgerKind::BuyIn { table: entry_table } if *entry_table == table => {
                    total - entry.delta
                }
                LedgerKind::CashOut { table: entry_table } if *entry_table == table => {
                    total - entry.delta
                }
                _ => total,
            })
            .max(0);
        let winnings = amount - staked;
        Self::append_locked(
            account,
            LedgerKind::CashOut { table },
            amount,
            "table cash-out".into(),
            0,
        );
        if matches!(&owner, AccountOwner::User(_)) && loan_count > 0 && winnings > 0 {
            let rate = loan_count as Cents;
            let fee = winnings / 100 * rate + winnings % 100 * rate / 100;
            if fee > 0 {
                Self::append_locked(
                    account,
                    LedgerKind::LoanInterest,
                    -fee,
                    format!("loan interest ({rate}%)"),
                    0,
                );
            }
        }
        let result = account.clone();
        self.persist(&result).await?;
        Ok(result)
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
    pub async fn blackjack_buy_in(
        &self,
        owner: AccountOwner,
        table: Uuid,
        amount: Cents,
    ) -> Result<Account, anyhow::Error> {
        if !valid_game_amount(amount) {
            return Err(anyhow::anyhow!("game entry must be between $1 and $10,000"));
        }
        self.append(
            owner,
            LedgerKind::BlackjackBuyIn { table },
            -amount,
            "blackjack table buy-in".into(),
        )
        .await
    }
    pub async fn blackjack_cash_out(
        &self,
        owner: AccountOwner,
        table: Uuid,
        amount: Cents,
    ) -> Result<Account, anyhow::Error> {
        if amount < 0 {
            return Err(anyhow::anyhow!("cash-out amount cannot be negative"));
        }
        if amount == 0 {
            return self.account(owner).await;
        }
        self.append(
            owner,
            LedgerKind::BlackjackCashOut { table },
            amount,
            "blackjack table cash-out".into(),
        )
        .await
    }
    /// Hands money from one account to another in whole $1,000 chips.
    ///
    /// Both ledgers move under one lock, so nobody can spend the same chips
    /// twice, and the pair of entries cancels out: the total on the books is
    /// unchanged (§V1). The two files are written debit first, so a crash
    /// between them can only lose the gift, never mint it.
    pub async fn transfer(
        &self,
        from: AccountOwner,
        to: AccountOwner,
        amount: Cents,
        from_memo: String,
        to_memo: String,
    ) -> Result<(Account, Account), anyhow::Error> {
        if from == to {
            return Err(anyhow::anyhow!("that account is your own"));
        }
        if !valid_gift_amount(amount) {
            return Err(anyhow::anyhow!(
                "money is sent in whole $1,000 chips, up to $1,000,000 at a time"
            ));
        }
        let mut guard = self.inner.lock().await;
        Self::ensure_account_locked(&mut guard.accounts, &from);
        Self::ensure_account_locked(&mut guard.accounts, &to);
        if guard.accounts[&from].balance < amount {
            return Err(anyhow::anyhow!("not enough to send that"));
        }
        Self::append_locked(
            guard.accounts.get_mut(&from).expect("account"),
            LedgerKind::Gift { peer: to.clone() },
            -amount,
            from_memo,
            0,
        );
        Self::append_locked(
            guard.accounts.get_mut(&to).expect("account"),
            LedgerKind::Gift { peer: from.clone() },
            amount,
            to_memo,
            0,
        );
        let sender = guard.accounts[&from].clone();
        let recipient = guard.accounts[&to].clone();
        self.persist(&sender).await?;
        self.persist(&recipient).await?;
        Ok((sender, recipient))
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
    fn ensure_account_locked(accounts: &mut HashMap<AccountOwner, Account>, owner: &AccountOwner) {
        accounts.entry(owner.clone()).or_insert_with(|| {
            let now = Utc::now();
            Account {
                owner: owner.clone(),
                balance: 0,
                loan_count: 0,
                entries: Vec::new(),
                created_at: now,
                updated_at: now,
            }
        });
    }
    fn append_locked(
        account: &mut Account,
        kind: LedgerKind,
        delta: Cents,
        memo: String,
        loan_count: u64,
    ) {
        account.balance += delta;
        account.updated_at = Utc::now();
        account.loan_count += loan_count;
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BotLoanForgivenessReport {
    pub accounts: usize,
    pub loans: u64,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn the_house_starts_over_but_people_keep_their_money() {
        let root = std::env::temp_dir().join(format!("two-seven-reset-{}", Uuid::new_v4()));
        let bot = AccountOwner::Bot(crate::table::Bot::new(crate::table::BotKind::Fish, 0));
        let person = AccountOwner::User(Uuid::new_v4());
        {
            let bank = BankStore::load(&root).await.unwrap();
            bank.re_up(bot.clone()).await.unwrap();
            bank.re_up(person.clone()).await.unwrap();
            let banked = bank.account(bot.clone()).await.unwrap();
            assert!(banked.balance > 0 && banked.loan_count > 0);
        }
        // Clearing the marker is what a release does when it bumps its name.
        tokio::fs::remove_file(root.join("bank").join(BankStore::HOUSE_RESET_MARKER))
            .await
            .unwrap();

        let bank = BankStore::load(&root).await.unwrap();
        let house = bank.account(bot).await.unwrap();
        assert_eq!(house.balance, 0, "no money");
        assert_eq!(house.loan_count, 0, "no loans");
        assert!(house.entries.is_empty(), "no history");
        assert_eq!(
            bank.account(person).await.unwrap().balance,
            BankStore::RE_UP_AMOUNT,
            "a person's account is left alone"
        );
    }

    #[tokio::test]
    async fn a_bot_gets_a_house_stake_then_loans_after_busting() {
        let root = std::env::temp_dir().join(format!("two-seven-bigseat-{}", Uuid::new_v4()));
        let bank = BankStore::load(&root).await.unwrap();
        let bot = AccountOwner::Bot(crate::table::Bot::new(crate::table::BotKind::Shark, 0));
        let table = Uuid::new_v4();
        let account = bank
            .buy_in(bot.clone(), table, 100_000_000, false)
            .await
            .unwrap();
        assert_eq!(account.loan_count, 0, "the first bot funding is not a loan");
        assert_eq!(account.entries.len(), 2, "one stake entry and a buy-in");
        assert_eq!(account.entries[0].delta, 100_000_000);
        assert_eq!(account.entries[0].kind, LedgerKind::HouseStake);
        assert_eq!(account.entries[0].memo, "house stake");
        assert_eq!(account.balance, 0);
        let account = bank.buy_in(bot, Uuid::new_v4(), 100, false).await.unwrap();
        assert_eq!(account.loan_count, 1);
        assert_eq!(account.entries[2].kind, LedgerKind::ReUp);
        assert_eq!(account.entries[2].memo, "re-up loan");
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
    async fn game_entries_validate_amounts_and_can_create_loans() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        assert!(
            bank.buy_in(owner.clone(), Uuid::new_v4(), 100_000_001, false)
                .await
                .is_err()
        );
        bank.buy_in(owner.clone(), Uuid::new_v4(), 100, false)
            .await
            .unwrap();
        let account = bank.account(owner).await.unwrap();
        assert_eq!(account.balance, BankStore::RE_UP_AMOUNT - 100);
        assert_eq!(account.loan_count, 1);
    }

    #[tokio::test]
    async fn re_up_requires_balance_below_one_thousand_and_tracks_loans() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        let account = bank.re_up(owner.clone()).await.unwrap();
        assert_eq!(account.balance, BankStore::RE_UP_AMOUNT);
        assert_eq!(account.loan_count, 1);
        bank.append(owner.clone(), LedgerKind::Adjustment, -1, "spend".into())
            .await
            .unwrap();
        let account = bank.account(owner.clone()).await.unwrap();
        assert_eq!(account_json(&account)["can_re_up"], true);
        let account = bank.re_up(owner.clone()).await.unwrap();
        assert_eq!(account.balance, BankStore::RE_UP_AMOUNT * 2 - 1);
        assert_eq!(account.loan_count, 2);
        assert_eq!(account_json(&account)["can_re_up"], false);
        bank.append(
            owner.clone(),
            LedgerKind::Adjustment,
            -(BankStore::RE_UP_AMOUNT - 1),
            "spend".into(),
        )
        .await
        .unwrap();
        assert_eq!(
            bank.account(owner.clone()).await.unwrap().balance,
            BankStore::RE_UP_AMOUNT
        );
        let account = bank.account(owner.clone()).await.unwrap();
        assert_eq!(account_json(&account)["can_re_up"], false);
        assert_eq!(
            bank.re_up(owner).await.unwrap_err().to_string(),
            "re-up is only available below $1,000"
        );
    }

    #[tokio::test]
    async fn repayment_decrements_loan_count_and_costs_one_thousand() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        bank.re_up(owner.clone()).await.unwrap();
        bank.append(
            owner.clone(),
            LedgerKind::Adjustment,
            -BankStore::RE_UP_AMOUNT,
            "spend".into(),
        )
        .await
        .unwrap();
        bank.re_up(owner.clone()).await.unwrap();
        let account = bank.repay_loan(owner).await.unwrap();
        assert_eq!(account.balance, 0);
        assert_eq!(account.loan_count, 1);
        assert_eq!(account.loan_debt(), BankStore::RE_UP_AMOUNT);
        assert_eq!(account.net_balance(), -BankStore::RE_UP_AMOUNT);
    }

    #[tokio::test]
    async fn repayment_requires_an_outstanding_loan_and_enough_balance() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        assert_eq!(
            bank.repay_loan(owner.clone())
                .await
                .unwrap_err()
                .to_string(),
            "no outstanding loans"
        );
        bank.re_up(owner.clone()).await.unwrap();
        bank.append(
            owner.clone(),
            LedgerKind::Adjustment,
            -(BankStore::RE_UP_AMOUNT - 1),
            "spend".into(),
        )
        .await
        .unwrap();
        assert_eq!(
            bank.repay_loan(owner).await.unwrap_err().to_string(),
            "not enough to pay back that loan"
        );
    }

    #[tokio::test]
    async fn a_large_stake_uses_one_entry_and_repays_loans_one_at_a_time() {
        // The lending limit is a rule about people; the house is staked at
        // every rung of the ladder, however deep.
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::Bot(crate::table::Bot::new(crate::table::BotKind::Shark, 0));
        let table = Uuid::new_v4();
        // The first shortfall is the house's own stake, not a loan.
        bank.buy_in(owner.clone(), table, 100_000_000, false)
            .await
            .unwrap();
        let account = bank
            .buy_in(owner.clone(), table, 100_000_000, false)
            .await
            .unwrap();
        assert_eq!(account.loan_count, 1_000);
        assert_eq!(account.loan_debt(), 100_000_000);
        assert_eq!(
            account
                .entries
                .iter()
                .filter(|entry| matches!(entry.kind, LedgerKind::ReUp))
                .count(),
            1,
            "one entry covers the whole loan"
        );
        let account = bank
            .append(
                owner.clone(),
                LedgerKind::Adjustment,
                100_000_000,
                "repayment funds".into(),
            )
            .await
            .unwrap();
        assert_eq!(account.balance, 100_000_000);
        let account = bank.repay_loan(owner).await.unwrap();
        assert_eq!(account.balance, 99_900_000);
        assert_eq!(account.loan_debt(), 99_900_000);
        assert_eq!(account.loan_count, 999);
    }

    #[tokio::test]
    async fn a_person_only_borrows_their_way_into_the_cheap_seats() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        let table = Uuid::new_v4();
        // One rung above the limit is money you have to bring yourself.
        assert_eq!(
            bank.buy_in(
                owner.clone(),
                table,
                BankStore::LOAN_BUY_IN_LIMIT + 100_000,
                false
            )
            .await
            .unwrap_err()
            .to_string(),
            "insufficient funds"
        );
        let account = bank.account(owner.clone()).await.unwrap();
        assert_eq!(account.loan_count, 0);
        assert_eq!(account.balance, 0);
        // At the limit it lends.
        let account = bank
            .buy_in(owner, table, BankStore::LOAN_BUY_IN_LIMIT, false)
            .await
            .unwrap();
        assert_eq!(account.loan_count, 1);
        assert_eq!(account.balance, 0);
    }

    #[tokio::test]
    async fn paying_off_every_loan_clears_what_the_balance_covers() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        for _ in 0..3 {
            bank.re_up(owner.clone()).await.unwrap();
            bank.append(
                owner.clone(),
                LedgerKind::Adjustment,
                -BankStore::RE_UP_AMOUNT,
                "spent it".into(),
            )
            .await
            .unwrap();
        }
        // Three loans owed, enough on hand for two of them.
        bank.append(
            owner.clone(),
            LedgerKind::Adjustment,
            2 * BankStore::RE_UP_AMOUNT + 500,
            "winnings".into(),
        )
        .await
        .unwrap();
        let account = bank.repay_all_loans(owner.clone()).await.unwrap();
        assert_eq!(account.loan_count, 1);
        assert_eq!(account.balance, 500);
        assert_eq!(
            account.entries.last().expect("entry").memo,
            "loan repayment (2 loans)"
        );
        assert_eq!(
            account.entries.last().expect("entry").delta,
            -2 * BankStore::RE_UP_AMOUNT
        );
        // Short of a whole loan it does nothing, and with none owed there is
        // nothing to do either.
        assert_eq!(
            bank.repay_all_loans(owner.clone())
                .await
                .unwrap_err()
                .to_string(),
            "not enough to pay back that loan"
        );
        bank.append(
            owner.clone(),
            LedgerKind::Adjustment,
            BankStore::RE_UP_AMOUNT,
            "winnings".into(),
        )
        .await
        .unwrap();
        let account = bank.repay_all_loans(owner.clone()).await.unwrap();
        assert_eq!(account.loan_count, 0);
        assert_eq!(account.balance, 500);
        assert_eq!(
            account.entries.last().expect("entry").memo,
            "loan repayment"
        );
        assert_eq!(
            bank.repay_all_loans(owner).await.unwrap_err().to_string(),
            "no outstanding loans"
        );
    }

    #[tokio::test]
    async fn forgiving_bot_loans_preserves_balances_and_human_loans() {
        let root = tempfile_dir();
        let bank = BankStore::load(&root).await.unwrap();
        let bot = AccountOwner::Bot(crate::table::Bot::new(crate::table::BotKind::Shark, 0));
        let human = AccountOwner::User(Uuid::new_v4());
        let bot_before = bank.re_up(bot.clone()).await.unwrap();
        let human_before = bank.re_up(human.clone()).await.unwrap();
        let report = bank.forgive_bot_loans().await.unwrap();
        assert_eq!(
            report,
            BotLoanForgivenessReport {
                accounts: 1,
                loans: 1
            }
        );
        let bot_after = bank.account(bot).await.unwrap();
        let human_after = bank.account(human).await.unwrap();
        assert_eq!(bot_after.balance, bot_before.balance);
        assert_eq!(bot_after.loan_count, 0);
        assert!(bot_after.updated_at >= bot_before.updated_at);
        assert_eq!(human_after.balance, human_before.balance);
        assert_eq!(human_after.loan_count, human_before.loan_count);
        let reloaded = BankStore::load(&root).await.unwrap();
        assert_eq!(
            reloaded
                .account(AccountOwner::Bot(crate::table::Bot::new(
                    crate::table::BotKind::Shark,
                    0
                )))
                .await
                .unwrap()
                .loan_count,
            0
        );
    }

    #[tokio::test]
    async fn cash_out_interest_is_based_on_winnings_and_capped() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        for _ in 0..12 {
            bank.re_up(owner.clone()).await.unwrap();
            bank.append(
                owner.clone(),
                LedgerKind::Adjustment,
                -BankStore::RE_UP_AMOUNT,
                "spend".into(),
            )
            .await
            .unwrap();
        }
        let table = Uuid::new_v4();
        bank.append(
            owner.clone(),
            LedgerKind::Adjustment,
            10_000,
            "stake".into(),
        )
        .await
        .unwrap();
        bank.buy_in(owner.clone(), table, 10_000, true)
            .await
            .unwrap();
        let account = bank.cash_out(owner, table, 20_000).await.unwrap();
        let interest = account.entries.last().unwrap();
        assert_eq!(interest.kind, LedgerKind::LoanInterest);
        assert_eq!(interest.delta, -1_000);
        assert_eq!(interest.memo, "loan interest (10%)");
        assert_eq!(account.balance, 19_000);
    }

    #[tokio::test]
    async fn cash_out_interest_skips_no_winnings_and_no_loans() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let no_loan = AccountOwner::User(Uuid::new_v4());
        let table = Uuid::new_v4();
        bank.append(
            no_loan.clone(),
            LedgerKind::Adjustment,
            10_000,
            "seed".into(),
        )
        .await
        .unwrap();
        bank.buy_in(no_loan.clone(), table, 10_000, true)
            .await
            .unwrap();
        let account = bank.cash_out(no_loan, table, 10_000).await.unwrap();
        assert!(
            !account
                .entries
                .iter()
                .any(|entry| matches!(entry.kind, LedgerKind::LoanInterest))
        );

        let owner = AccountOwner::User(Uuid::new_v4());
        bank.re_up(owner.clone()).await.unwrap();
        bank.append(
            owner.clone(),
            LedgerKind::Adjustment,
            -BankStore::RE_UP_AMOUNT + 10_000,
            "seed".into(),
        )
        .await
        .unwrap();
        bank.buy_in(owner.clone(), table, 10_000, true)
            .await
            .unwrap();
        let account = bank.cash_out(owner, table, 10_000).await.unwrap();
        assert!(
            !account
                .entries
                .iter()
                .any(|entry| matches!(entry.kind, LedgerKind::LoanInterest))
        );
    }

    #[tokio::test]
    async fn cash_out_interest_skips_bots() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::Bot(crate::table::Bot::new(crate::table::BotKind::Shark, 0));
        let table = Uuid::new_v4();
        bank.re_up(owner.clone()).await.unwrap();
        bank.append(
            owner.clone(),
            LedgerKind::Adjustment,
            10_000,
            "stake".into(),
        )
        .await
        .unwrap();
        bank.buy_in(owner.clone(), table, 10_000, true)
            .await
            .unwrap();
        let account = bank.cash_out(owner, table, 20_000).await.unwrap();
        assert_eq!(account.balance, 120_000);
        assert!(
            !account
                .entries
                .iter()
                .any(|entry| matches!(entry.kind, LedgerKind::LoanInterest))
        );
    }

    #[tokio::test]
    async fn cash_out_winnings_account_for_rebuys_at_the_same_table() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        bank.re_up(owner.clone()).await.unwrap();
        let table = Uuid::new_v4();
        bank.buy_in(owner.clone(), table, 10_000, false)
            .await
            .unwrap();
        bank.cash_out(owner.clone(), table, 5_000).await.unwrap();
        bank.append(owner.clone(), LedgerKind::Adjustment, 5_000, "seed".into())
            .await
            .unwrap();
        bank.buy_in(owner.clone(), table, 10_000, true)
            .await
            .unwrap();
        let account = bank.cash_out(owner, table, 25_000).await.unwrap();
        let interest = account.entries.last().unwrap();
        assert_eq!(interest.kind, LedgerKind::LoanInterest);
        assert_eq!(interest.delta, -100);
        assert_eq!(interest.memo, "loan interest (1%)");
    }

    #[tokio::test]
    async fn repayment_and_interest_preserve_ledger_balances() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        bank.re_up(owner.clone()).await.unwrap();
        let account = bank.repay_loan(owner.clone()).await.unwrap();
        assert_eq!(account.balance, 0);
        bank.re_up(owner.clone()).await.unwrap();
        let table = Uuid::new_v4();
        bank.buy_in(owner.clone(), table, 10_000, false)
            .await
            .unwrap();
        let account = bank.cash_out(owner, table, 20_000).await.unwrap();
        assert_eq!(
            account.balance,
            account
                .entries
                .iter()
                .map(|entry| entry.delta)
                .sum::<Cents>()
        );
        assert!(account.entries.iter().enumerate().all(|(index, entry)| {
            entry.balance_after
                == account.entries[..=index]
                    .iter()
                    .map(|item| item.delta)
                    .sum::<Cents>()
        }));
    }

    #[tokio::test]
    async fn buy_ins_auto_re_up_without_debt() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        let account = bank
            .buy_in(owner, Uuid::new_v4(), 100, false)
            .await
            .unwrap();
        assert_eq!(account.balance, BankStore::RE_UP_AMOUNT - 100);
        assert_eq!(account.loan_count, 1);
        assert_eq!(account.entries[0].kind, LedgerKind::ReUp);
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

    #[tokio::test]
    async fn a_gift_moves_whole_thousands_and_keeps_the_books_level() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let giver = AccountOwner::User(Uuid::new_v4());
        let taker = AccountOwner::User(Uuid::new_v4());
        bank.re_up(giver.clone()).await.unwrap();
        bank.append(
            giver.clone(),
            LedgerKind::Adjustment,
            BankStore::GIFT_INCREMENT,
            "seed".into(),
        )
        .await
        .unwrap();
        let before = bank.account(giver.clone()).await.unwrap().balance;

        let (sender, recipient) = bank
            .transfer(
                giver.clone(),
                taker.clone(),
                BankStore::GIFT_INCREMENT,
                "gift to Taker".into(),
                "gift from Giver".into(),
            )
            .await
            .unwrap();

        assert_eq!(sender.balance, before - BankStore::GIFT_INCREMENT);
        assert_eq!(recipient.balance, BankStore::GIFT_INCREMENT);
        assert_eq!(
            sender.balance + recipient.balance,
            before,
            "V1: a gift moves money, it does not make any"
        );
        assert_eq!(
            sender.entries.last().unwrap().kind,
            LedgerKind::Gift {
                peer: taker.clone()
            }
        );
        assert_eq!(
            recipient.entries.last().unwrap().kind,
            LedgerKind::Gift {
                peer: giver.clone()
            }
        );
        assert_eq!(recipient.entries.last().unwrap().memo, "gift from Giver");
        assert_eq!(sender.loan_count, 1, "giving money away repays nothing");
        // V2 holds on both sides of the transfer.
        for account in [&sender, &recipient] {
            assert_eq!(
                account
                    .entries
                    .iter()
                    .map(|entry| entry.delta)
                    .sum::<Cents>(),
                account.balance
            );
        }
    }

    #[tokio::test]
    async fn gifts_net_out_per_counterparty_biggest_giving_first() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let me = AccountOwner::User(Uuid::new_v4());
        let generous = AccountOwner::User(Uuid::new_v4());
        let stingy = AccountOwner::User(Uuid::new_v4());
        bank.append(me.clone(), LedgerKind::Adjustment, 500_000, "seed".into())
            .await
            .unwrap();
        for peer in [&generous, &stingy] {
            bank.append(peer.clone(), LedgerKind::Adjustment, 500_000, "seed".into())
                .await
                .unwrap();
        }
        let send = async |from: &AccountOwner, to: &AccountOwner, amount| {
            bank.transfer(from.clone(), to.clone(), amount, "out".into(), "in".into())
                .await
                .unwrap();
        };
        send(&generous, &me, 300_000).await;
        send(&me, &generous, 100_000).await;
        send(&me, &stingy, 200_000).await;
        send(&stingy, &me, 100_000).await;

        let tallies = bank.account(me.clone()).await.unwrap().gift_tallies();
        assert_eq!(tallies.len(), 2, "one row per person, not per gift");
        assert_eq!(
            tallies[0].peer, stingy,
            "whoever got the most out of me leads"
        );
        assert_eq!(tallies[0].received, 100_000);
        assert_eq!(tallies[0].sent, 200_000);
        assert_eq!(
            tallies[0].net(),
            100_000,
            "giving more than you got is positive"
        );
        assert_eq!(tallies[1].peer, generous);
        assert_eq!(tallies[1].net(), -200_000, "V58: the pair cancels out");

        let theirs = bank.account(generous).await.unwrap().gift_tallies();
        assert_eq!(theirs[0].peer, me);
        assert_eq!(theirs[0].net(), 200_000, "each side is the other's mirror");

        assert!(
            bank.account(AccountOwner::User(Uuid::new_v4()))
                .await
                .unwrap()
                .gift_tallies()
                .is_empty(),
            "an account that has never traded gifts has no counterparties"
        );
    }

    #[tokio::test]
    async fn a_gift_is_refused_below_a_thousand_off_the_increment_or_over_budget() {
        let bank = BankStore::load(tempfile_dir()).await.unwrap();
        let giver = AccountOwner::User(Uuid::new_v4());
        let taker = AccountOwner::User(Uuid::new_v4());
        bank.re_up(giver.clone()).await.unwrap();
        let gift = async |amount| {
            bank.transfer(
                giver.clone(),
                taker.clone(),
                amount,
                "gift".into(),
                "gift".into(),
            )
            .await
        };

        assert!(gift(0).await.is_err(), "nothing is not a gift");
        assert!(gift(-BankStore::GIFT_INCREMENT).await.is_err());
        assert!(gift(50_000).await.is_err(), "half a chip is not a chip");
        assert!(gift(150_000).await.is_err(), "nor is a chip and a half");
        assert!(
            gift(BankStore::MAX_GIFT + BankStore::GIFT_INCREMENT)
                .await
                .is_err()
        );
        assert!(
            gift(BankStore::GIFT_INCREMENT * 2).await.is_err(),
            "you cannot give away what you do not have"
        );
        assert!(
            bank.transfer(
                giver.clone(),
                giver.clone(),
                BankStore::GIFT_INCREMENT,
                "gift".into(),
                "gift".into()
            )
            .await
            .is_err(),
            "your own account is not somebody else"
        );
        assert_eq!(
            bank.account(giver).await.unwrap().balance,
            BankStore::RE_UP_AMOUNT,
            "a refused gift leaves the account alone"
        );
        assert_eq!(bank.account(taker).await.unwrap().balance, 0);
    }
}

#[cfg(test)]
mod no_debt_tests {
    use super::*;
    #[tokio::test]
    async fn buy_in_only_requires_balance_when_no_debt_is_true() {
        let bank = BankStore::load(
            std::env::temp_dir().join(format!("two-seven-no-debt-{}", Uuid::new_v4())),
        )
        .await
        .unwrap();
        let owner = AccountOwner::User(Uuid::new_v4());
        assert_eq!(
            bank.buy_in(owner.clone(), Uuid::new_v4(), 100, false)
                .await
                .unwrap()
                .balance,
            BankStore::RE_UP_AMOUNT - 100
        );
        let no_debt_owner = AccountOwner::User(Uuid::new_v4());
        bank.append(
            no_debt_owner.clone(),
            LedgerKind::Adjustment,
            100,
            "seed".into(),
        )
        .await
        .unwrap();
        assert!(
            bank.buy_in(no_debt_owner.clone(), Uuid::new_v4(), 101, true)
                .await
                .is_err()
        );
        assert_eq!(
            bank.buy_in(no_debt_owner, Uuid::new_v4(), 100, true)
                .await
                .unwrap()
                .balance,
            0
        );
    }
}
