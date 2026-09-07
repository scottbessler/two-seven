//! Roulette: the board, the bets it allows, and what they pay.
//!
//! A roulette bet is a chip resting somewhere on the felt, and where it rests
//! is the whole rule: the middle of a square is one number, the line between
//! two squares is both of them, the cross where four meet is all four. So the
//! board is described once, geometrically, and every legal bet is *derived*
//! from that description into a catalogue the server builds at startup.
//!
//! Nothing else validates bets. A request names a bet by id and either the
//! catalogue has it or the request is refused, which means a bet the geometry
//! cannot produce cannot be placed, priced, or paid — there is no second list
//! of odds to fall out of step with the first.
//!
//! The wheel is single-zero, so the house edge is one pocket in thirty-seven
//! (2.70%) on every bet here, inside and outside alike. That falls out of the
//! payouts rather than being applied anywhere: it is the gap between the 37
//! pockets and the 36-to-1 a straight-up would have to pay to be fair.

use crate::money::Cents;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The pockets, in the order they sit on a European wheel. The board does not
/// care about this order, but the animation and the wheel itself do.
pub const POCKETS: u8 = 37;

/// Reds on a European wheel. Everything else but the zero is black.
const RED: [u8; 18] = [
    1, 3, 5, 7, 9, 12, 14, 16, 18, 19, 21, 23, 25, 27, 30, 32, 34, 36,
];

/// The felt: twelve rows of three, with the zero above them. Row `r` carries
/// `3r+1`, `3r+2`, `3r+3`, so a column is one arithmetic progression and a
/// street is one row -- which is all the geometry the catalogue below needs.
pub const ROWS: u8 = 12;
pub const COLUMNS: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Colour {
    Green,
    Red,
    Black,
}

pub fn colour(number: u8) -> Colour {
    if number == 0 {
        Colour::Green
    } else if RED.contains(&number) {
        Colour::Red
    } else {
        Colour::Black
    }
}

/// The shapes a chip can make on the felt. The name is what the player is told
/// they have bet; the odds come with it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BetKind {
    Straight,
    Split,
    Street,
    Corner,
    Line,
    Trio,
    Basket,
    Dozen,
    Column,
    Red,
    Black,
    Odd,
    Even,
    Low,
    High,
}

impl BetKind {
    /// What a winning chip is paid, to one. A player gets this many times the
    /// stake *plus the stake back*, which is what `returns` works out.
    pub const fn payout(self) -> Cents {
        match self {
            BetKind::Straight => 35,
            BetKind::Split => 17,
            BetKind::Street | BetKind::Trio => 11,
            BetKind::Corner | BetKind::Basket => 8,
            BetKind::Line => 5,
            BetKind::Dozen | BetKind::Column => 2,
            BetKind::Red
            | BetKind::Black
            | BetKind::Odd
            | BetKind::Even
            | BetKind::Low
            | BetKind::High => 1,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            BetKind::Straight => "Straight up",
            BetKind::Split => "Split",
            BetKind::Street => "Street",
            BetKind::Corner => "Corner",
            BetKind::Line => "Six line",
            BetKind::Trio => "Trio",
            BetKind::Basket => "First four",
            BetKind::Dozen => "Dozen",
            BetKind::Column => "Column",
            BetKind::Red => "Red",
            BetKind::Black => "Black",
            BetKind::Odd => "Odd",
            BetKind::Even => "Even",
            BetKind::Low => "1 to 18",
            BetKind::High => "19 to 36",
        }
    }
}

/// One legal resting place for a chip. The odds and the name travel with it,
/// so the page can price and label a bet without a second table to fall out of
/// step with this one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BetSpec {
    pub id: String,
    pub kind: BetKind,
    /// Sorted, and never empty.
    pub numbers: Vec<u8>,
    pub payout: Cents,
    pub label: &'static str,
}

impl BetSpec {
    /// What a winning stake comes back as: the winnings and the stake itself.
    pub fn returns(&self, stake: Cents) -> Cents {
        stake * (self.kind.payout() + 1)
    }

    pub fn covers(&self, number: u8) -> bool {
        self.numbers.contains(&number)
    }
}

fn cell(row: u8, column: u8) -> u8 {
    row * COLUMNS + column + 1
}

fn spec(kind: BetKind, numbers: Vec<u8>) -> BetSpec {
    let id = match kind {
        BetKind::Red => "red".to_string(),
        BetKind::Black => "black".to_string(),
        BetKind::Odd => "odd".to_string(),
        BetKind::Even => "even".to_string(),
        BetKind::Low => "low".to_string(),
        BetKind::High => "high".to_string(),
        BetKind::Dozen => format!("dozen:{}", numbers[0] / 12 + 1),
        BetKind::Column => format!("column:{}", (numbers[0] - 1) % COLUMNS + 1),
        // Every other shape is named by exactly what it covers, so an id can be
        // read -- in a request log, a test, or a bug report -- without a table.
        _ => format!(
            "{}:{}",
            match kind {
                BetKind::Straight => "straight",
                BetKind::Split => "split",
                BetKind::Street => "street",
                BetKind::Corner => "corner",
                BetKind::Line => "line",
                BetKind::Trio => "trio",
                _ => "basket",
            },
            numbers
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join("-")
        ),
    };
    BetSpec {
        id,
        kind,
        numbers,
        payout: kind.payout(),
        label: kind.label(),
    }
}

/// Every bet the felt allows, built once from the geometry above.
fn build_catalogue() -> BTreeMap<String, BetSpec> {
    let mut specs = Vec::new();

    for number in 0..POCKETS {
        specs.push(spec(BetKind::Straight, vec![number]));
    }

    for row in 0..ROWS {
        // Along a row: two splits, and the row itself is a street.
        for column in 0..COLUMNS - 1 {
            specs.push(spec(
                BetKind::Split,
                vec![cell(row, column), cell(row, column + 1)],
            ));
        }
        specs.push(spec(
            BetKind::Street,
            (0..COLUMNS).map(|column| cell(row, column)).collect(),
        ));
        if row + 1 == ROWS {
            continue;
        }
        // Down the board: a split per column, a corner per pair of columns,
        // and the six line across both rows.
        for column in 0..COLUMNS {
            specs.push(spec(
                BetKind::Split,
                vec![cell(row, column), cell(row + 1, column)],
            ));
        }
        for column in 0..COLUMNS - 1 {
            specs.push(spec(
                BetKind::Corner,
                vec![
                    cell(row, column),
                    cell(row, column + 1),
                    cell(row + 1, column),
                    cell(row + 1, column + 1),
                ],
            ));
        }
        specs.push(spec(
            BetKind::Line,
            (0..COLUMNS)
                .flat_map(|column| [cell(row, column), cell(row + 1, column)])
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
        ));
    }

    // The zero touches the first row along its whole edge, which is what gives
    // it three splits, two trios and the first four.
    for number in 1..=COLUMNS {
        specs.push(spec(BetKind::Split, vec![0, number]));
    }
    specs.push(spec(BetKind::Trio, vec![0, 1, 2]));
    specs.push(spec(BetKind::Trio, vec![0, 2, 3]));
    specs.push(spec(BetKind::Basket, vec![0, 1, 2, 3]));

    for dozen in 0..3u8 {
        specs.push(spec(
            BetKind::Dozen,
            (1..=12).map(|n| dozen * 12 + n).collect(),
        ));
    }
    for column in 0..COLUMNS {
        specs.push(spec(
            BetKind::Column,
            (0..ROWS).map(|row| cell(row, column)).collect(),
        ));
    }

    let evens = [
        (
            BetKind::Red,
            (1..POCKETS).filter(|n| RED.contains(n)).collect::<Vec<_>>(),
        ),
        (
            BetKind::Black,
            (1..POCKETS).filter(|n| !RED.contains(n)).collect(),
        ),
        (BetKind::Odd, (1..POCKETS).filter(|n| n % 2 == 1).collect()),
        (BetKind::Even, (1..POCKETS).filter(|n| n % 2 == 0).collect()),
        (BetKind::Low, (1..=18).collect()),
        (BetKind::High, (19..POCKETS).collect()),
    ];
    for (kind, numbers) in evens {
        specs.push(spec(kind, numbers));
    }

    let mut catalogue = BTreeMap::new();
    for mut entry in specs {
        entry.numbers.sort_unstable();
        debug_assert!(
            !catalogue.contains_key(&entry.id),
            "duplicate bet id {}",
            entry.id
        );
        catalogue.insert(entry.id.clone(), entry);
    }
    catalogue
}

pub fn catalogue() -> &'static BTreeMap<String, BetSpec> {
    static CATALOGUE: OnceLock<BTreeMap<String, BetSpec>> = OnceLock::new();
    CATALOGUE.get_or_init(build_catalogue)
}

/// The bet with this id, or nothing. This is the only door into the catalogue,
/// so an unknown id is refused everywhere by construction.
pub fn bet(id: &str) -> Option<&'static BetSpec> {
    catalogue().get(id)
}

// ---------------------------------------------------------------------------
// The table
// ---------------------------------------------------------------------------

/// What a seat is bought for, and the most that may rest on any one spot. The
/// spot cap is what bounds the table's exposure: thirty-five to one on the cap
/// is the largest single payout the house can owe.
pub const BUY_IN: Cents = 100_000;
pub const MAX_SPOT: Cents = 20_000;
/// The chips in the tray. Every stake is a whole number of these.
pub const CHIPS: [Cents; 4] = [100, 500, 2_500, 10_000];
/// How many chips may rest on the felt at once. A real table is bounded by the
/// dealer's patience; this one is bounded so the state stays a fixed size.
pub const MAX_CHIPS: usize = 60;
/// Numbers kept on the marquee. Bounded for the same reason (§V64).
pub const HISTORY: usize = 24;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouletteError {
    UnknownBet,
    NotAChip,
    SpotLimit,
    Crowded,
    Broke,
    NoBets,
    NotSeated,
}

impl RouletteError {
    pub const fn message(self) -> &'static str {
        match self {
            RouletteError::UnknownBet => "There is no such bet on this table.",
            RouletteError::NotAChip => "Bet with a chip from the tray.",
            RouletteError::SpotLimit => "That spot is at its limit.",
            RouletteError::Crowded => "There is no room left on the felt.",
            RouletteError::Broke => "You do not have the chips for that.",
            RouletteError::NoBets => "There is nothing on the felt.",
            RouletteError::NotSeated => "You are not at this table.",
        }
    }
}

/// One chip, where it rests. Kept in the order they were placed so the last
/// one can be lifted again -- which is what an undo is at a real table.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Chip {
    pub bet: String,
    pub amount: Cents,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Win {
    pub bet: String,
    pub staked: Cents,
    pub returned: Cents,
}

/// What one spin did. The seed travels with it so the wheel on the client can
/// be replayed exactly -- reload mid-animation and the ball lands where it
/// already landed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Spin {
    pub number: u8,
    pub colour: Colour,
    pub seed: u32,
    pub staked: Cents,
    pub returned: Cents,
    pub wins: Vec<Win>,
    /// What was on the felt, so the same layout can be put back down.
    pub chips: Vec<Chip>,
    pub at: chrono::DateTime<chrono::Utc>,
}

impl Spin {
    pub fn net(&self) -> Cents {
        self.returned - self.staked
    }
}

/// One player's table. Roulette is played against the house rather than the
/// other players, so a table is a private thing here: the felt, the chips on
/// it, and the stack they came from.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouletteTable {
    pub id: uuid::Uuid,
    pub owner: uuid::Uuid,
    pub stack: Cents,
    /// Chips resting on the felt. These are a claim on `stack`, not a
    /// withdrawal from it: nothing is lost until the wheel is spun, so the
    /// table's whole worth is always exactly `stack` (§V1).
    pub chips: Vec<Chip>,
    pub last: Option<Spin>,
    pub history: Vec<u8>,
    pub spins: u64,
}

/// A player's table id. Folding the owner into a fixed namespace keeps it
/// stable across restarts without a lookup, and keeps it distinct from the
/// owner's own id so the two can never be confused in the ledger.
pub fn table_id(owner: uuid::Uuid) -> uuid::Uuid {
    const NAMESPACE: u128 = 0x726f_756c_0000_4b00_8000_0000_0000_0000;
    uuid::Uuid::from_u128(owner.as_u128() ^ NAMESPACE)
}

impl RouletteTable {
    pub fn new(owner: uuid::Uuid) -> Self {
        Self {
            // Derived from the owner so a table survives a restart under the
            // same id, and so the ledger's buy-in lines keep pointing at it.
            id: table_id(owner),
            owner,
            stack: 0,
            chips: Vec::new(),
            last: None,
            history: Vec::new(),
            spins: 0,
        }
    }

    /// Everything currently claimed by chips on the felt.
    pub fn staked(&self) -> Cents {
        self.chips.iter().map(|chip| chip.amount).sum()
    }

    /// What is left to bet with.
    pub fn available(&self) -> Cents {
        self.stack - self.staked()
    }

    pub fn on_spot(&self, bet: &str) -> Cents {
        self.chips
            .iter()
            .filter(|chip| chip.bet == bet)
            .map(|chip| chip.amount)
            .sum()
    }

    pub fn place(&mut self, bet_id: &str, amount: Cents) -> Result<(), RouletteError> {
        let spec = crate::roulette::bet(bet_id).ok_or(RouletteError::UnknownBet)?;
        if !CHIPS.contains(&amount) {
            return Err(RouletteError::NotAChip);
        }
        if self.chips.len() >= MAX_CHIPS {
            return Err(RouletteError::Crowded);
        }
        if self.on_spot(&spec.id) + amount > MAX_SPOT {
            return Err(RouletteError::SpotLimit);
        }
        if amount > self.available() {
            return Err(RouletteError::Broke);
        }
        self.chips.push(Chip {
            bet: spec.id.clone(),
            amount,
        });
        Ok(())
    }

    /// Lift the last chip placed.
    pub fn undo(&mut self) -> Option<Chip> {
        self.chips.pop()
    }

    pub fn clear(&mut self) -> Cents {
        let staked = self.staked();
        self.chips.clear();
        staked
    }

    /// Put the last spin's chips back down. This is the button people actually
    /// use between spins, so it is all-or-nothing: a rebet that silently
    /// placed half the layout would cost more than one that refused outright.
    pub fn rebet(&mut self) -> Result<(), RouletteError> {
        let previous = match &self.last {
            Some(spin) if !spin.chips.is_empty() => spin.chips.clone(),
            _ => return Err(RouletteError::NoBets),
        };
        if previous.iter().map(|chip| chip.amount).sum::<Cents>() > self.available() {
            return Err(RouletteError::Broke);
        }
        let restored = self.chips.clone();
        for chip in previous {
            if let Err(error) = self.place(&chip.bet, chip.amount) {
                self.chips = restored;
                return Err(error);
            }
        }
        Ok(())
    }

    /// Spin, settle, and hand back what happened. The number comes from the
    /// caller so the wheel's randomness lives in one place and a test can name
    /// the pocket it wants.
    pub fn spin(
        &mut self,
        number: u8,
        seed: u32,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Spin, RouletteError> {
        if self.chips.is_empty() {
            return Err(RouletteError::NoBets);
        }
        debug_assert!(number < POCKETS);
        let staked = self.staked();
        let placed = self.chips.clone();
        let mut wins: Vec<Win> = Vec::new();
        let mut returned = 0;
        for chip in &self.chips {
            let Some(spec) = crate::roulette::bet(&chip.bet) else {
                continue;
            };
            if !spec.covers(number) {
                continue;
            }
            let back = spec.returns(chip.amount);
            returned += back;
            match wins.iter_mut().find(|win| win.bet == chip.bet) {
                Some(win) => {
                    win.staked += chip.amount;
                    win.returned += back;
                }
                None => wins.push(Win {
                    bet: chip.bet.clone(),
                    staked: chip.amount,
                    returned: back,
                }),
            }
        }
        // The stack pays for the felt and takes back what won. Doing both in
        // one step is what keeps the table worth exactly `stack` at every
        // moment a reader can observe it.
        self.stack += returned - staked;
        self.chips.clear();
        self.spins += 1;
        self.history.insert(0, number);
        self.history.truncate(HISTORY);
        let spin = Spin {
            number,
            colour: colour(number),
            seed,
            staked,
            returned,
            wins,
            chips: placed,
            at: now,
        };
        self.last = Some(spin.clone());
        Ok(spin)
    }
}

// ---------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------

/// What the page needs to draw itself. The catalogue is not in here: the board
/// is fixed, so the client knows the shapes and only the money changes.
#[derive(Clone, Debug, Serialize)]
pub struct RouletteView {
    pub table: uuid::Uuid,
    pub stack: Cents,
    pub staked: Cents,
    pub available: Cents,
    /// One entry per occupied spot, in the order first bet, so the board can
    /// draw a stack of chips wherever there is one.
    pub spots: Vec<Chip>,
    pub last: Option<Spin>,
    pub history: Vec<u8>,
    pub spins: u64,
    pub bank_balance: Cents,
    pub buy_in: Cents,
    pub max_spot: Cents,
    pub chips: [Cents; 4],
    pub max_chips: usize,
}

#[derive(Clone)]
pub struct RouletteStore {
    tables:
        std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<uuid::Uuid, RouletteTable>>>,
    path: Option<std::path::PathBuf>,
}

impl Default for RouletteStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RouletteStore {
    pub fn new() -> Self {
        Self {
            tables: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            path: None,
        }
    }

    pub async fn load(root: impl AsRef<std::path::Path>) -> Result<Self, anyhow::Error> {
        let dir = root.as_ref().join("roulette");
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join("tables.json");
        let tables: Vec<RouletteTable> = match tokio::fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        let mut map = std::collections::HashMap::new();
        for mut table in tables {
            // Chips on the felt are a claim on the stack rather than a
            // withdrawal from it, so sweeping them costs nobody anything and
            // means a restart never leaves a spin half-placed.
            table.chips.clear();
            map.insert(table.owner, table);
        }
        Ok(Self {
            tables: std::sync::Arc::new(tokio::sync::Mutex::new(map)),
            path: Some(path),
        })
    }

    async fn persist(&self) -> Result<(), anyhow::Error> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let tables: Vec<RouletteTable> = self.tables.lock().await.values().cloned().collect();
        let body = serde_json::to_vec_pretty(&tables)?;
        let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
        tokio::fs::write(&tmp, body).await?;
        tokio::fs::rename(tmp, path).await?;
        Ok(())
    }

    /// Every table that exists, for the conservation check and for tests.
    pub async fn stacks(&self) -> Cents {
        self.tables.lock().await.values().map(|t| t.stack).sum()
    }

    pub async fn view(
        &self,
        owner: uuid::Uuid,
        bank: &crate::bank::BankStore,
    ) -> Result<RouletteView, anyhow::Error> {
        let balance = bank
            .seat_bank(crate::bank::AccountOwner::User(owner))
            .await?
            .balance;
        let tables = self.tables.lock().await;
        let table = tables
            .get(&owner)
            .cloned()
            .unwrap_or_else(|| RouletteTable::new(owner));
        Ok(Self::render(&table, balance))
    }

    fn render(table: &RouletteTable, bank_balance: Cents) -> RouletteView {
        let mut spots: Vec<Chip> = Vec::new();
        for chip in &table.chips {
            match spots.iter_mut().find(|spot| spot.bet == chip.bet) {
                Some(spot) => spot.amount += chip.amount,
                None => spots.push(chip.clone()),
            }
        }
        RouletteView {
            table: table.id,
            stack: table.stack,
            staked: table.staked(),
            available: table.available(),
            spots,
            last: table.last.clone(),
            history: table.history.clone(),
            spins: table.spins,
            bank_balance,
            buy_in: BUY_IN,
            max_spot: MAX_SPOT,
            chips: CHIPS,
            max_chips: MAX_CHIPS,
        }
    }

    /// Run `action` against the player's table, then persist and re-render.
    async fn act(
        &self,
        owner: uuid::Uuid,
        bank: &crate::bank::BankStore,
        action: impl FnOnce(&mut RouletteTable) -> Result<(), RouletteError>,
    ) -> Result<RouletteView, RouletteError> {
        {
            let mut tables = self.tables.lock().await;
            let table = tables
                .entry(owner)
                .or_insert_with(|| RouletteTable::new(owner));
            action(table)?;
        }
        if let Err(error) = self.persist().await {
            tracing::error!(%error, "could not persist the roulette table");
        }
        self.view(owner, bank)
            .await
            .map_err(|_| RouletteError::NotSeated)
    }

    pub async fn place(
        &self,
        owner: uuid::Uuid,
        bank: &crate::bank::BankStore,
        bet: &str,
        amount: Cents,
    ) -> Result<RouletteView, RouletteError> {
        self.act(owner, bank, |table| table.place(bet, amount))
            .await
    }

    pub async fn undo(
        &self,
        owner: uuid::Uuid,
        bank: &crate::bank::BankStore,
    ) -> Result<RouletteView, RouletteError> {
        self.act(owner, bank, |table| {
            table.undo().map(|_| ()).ok_or(RouletteError::NoBets)
        })
        .await
    }

    pub async fn clear(
        &self,
        owner: uuid::Uuid,
        bank: &crate::bank::BankStore,
    ) -> Result<RouletteView, RouletteError> {
        self.act(owner, bank, |table| {
            table.clear();
            Ok(())
        })
        .await
    }

    pub async fn rebet(
        &self,
        owner: uuid::Uuid,
        bank: &crate::bank::BankStore,
    ) -> Result<RouletteView, RouletteError> {
        self.act(owner, bank, RouletteTable::rebet).await
    }

    /// Buy chips with money from the bank. The ledger line is the only place
    /// this money exists twice, and it is written before the chips appear, so
    /// a failure between the two loses the player nothing.
    pub async fn buy_in(
        &self,
        owner: uuid::Uuid,
        bank: &crate::bank::BankStore,
        amount: Cents,
    ) -> Result<RouletteView, anyhow::Error> {
        if amount < 1 {
            return Err(anyhow::anyhow!("a buy-in must be positive"));
        }
        let id = table_id(owner);
        bank.roulette_buy_in(crate::bank::AccountOwner::User(owner), id, amount)
            .await?;
        {
            let mut tables = self.tables.lock().await;
            let table = tables
                .entry(owner)
                .or_insert_with(|| RouletteTable::new(owner));
            table.stack += amount;
        }
        self.persist().await?;
        self.view(owner, bank).await
    }

    /// Take the stack back to the bank. The felt is swept first: chips resting
    /// on it are still the player's, so leaving them behind would lose money.
    pub async fn cash_out(
        &self,
        owner: uuid::Uuid,
        bank: &crate::bank::BankStore,
    ) -> Result<RouletteView, anyhow::Error> {
        let (id, amount) = {
            let mut tables = self.tables.lock().await;
            let Some(table) = tables.get_mut(&owner) else {
                return Err(anyhow::anyhow!("you are not at a roulette table"));
            };
            table.clear();
            let amount = std::mem::take(&mut table.stack);
            (table.id, amount)
        };
        if amount > 0 {
            bank.roulette_cash_out(crate::bank::AccountOwner::User(owner), id, amount)
                .await?;
        }
        self.persist().await?;
        self.view(owner, bank).await
    }

    /// Spin the wheel. The pocket is drawn here, once, from the system's
    /// generator -- the client is told what came up and never asked.
    pub async fn spin(
        &self,
        owner: uuid::Uuid,
        bank: &crate::bank::BankStore,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<RouletteView, RouletteError> {
        use rand::Rng;
        let (number, seed) = {
            let mut rng = rand::thread_rng();
            (rng.gen_range(0..POCKETS), rng.r#gen::<u32>())
        };
        self.act(owner, bank, |table| {
            table.spin(number, seed, now).map(|_| ())
        })
        .await
    }
}

#[cfg(test)]
mod board_tests {
    use super::*;
    use std::collections::HashSet;

    fn of_kind(kind: BetKind) -> Vec<&'static BetSpec> {
        catalogue()
            .values()
            .filter(|spec| spec.kind == kind)
            .collect()
    }

    #[test]
    fn the_felt_offers_exactly_the_bets_a_european_table_offers() {
        // Counted off a real layout: if the geometry above ever drifts, it
        // shows up here as a shape that gained or lost a resting place.
        let counts = [
            (BetKind::Straight, 37),
            // 12 rows x 2 across, 11 row-pairs x 3 down, and 3 against the zero.
            (BetKind::Split, 24 + 33 + 3),
            (BetKind::Street, 12),
            (BetKind::Corner, 22),
            (BetKind::Line, 11),
            (BetKind::Trio, 2),
            (BetKind::Basket, 1),
            (BetKind::Dozen, 3),
            (BetKind::Column, 3),
            (BetKind::Red, 1),
            (BetKind::Black, 1),
            (BetKind::Odd, 1),
            (BetKind::Even, 1),
            (BetKind::Low, 1),
            (BetKind::High, 1),
        ];
        for (kind, expected) in counts {
            assert_eq!(of_kind(kind).len(), expected, "{kind:?}");
        }
        assert_eq!(
            catalogue().len(),
            counts.iter().map(|(_, n)| n).sum::<usize>()
        );
    }

    #[test]
    fn every_bet_covers_the_right_number_of_pockets() {
        for spec in catalogue().values() {
            let expected = match spec.kind {
                BetKind::Straight => 1,
                BetKind::Split => 2,
                BetKind::Street | BetKind::Trio => 3,
                BetKind::Corner | BetKind::Basket => 4,
                BetKind::Line => 6,
                BetKind::Dozen | BetKind::Column => 12,
                _ => 18,
            };
            assert_eq!(spec.numbers.len(), expected, "{}", spec.id);
            let unique: HashSet<_> = spec.numbers.iter().collect();
            assert_eq!(unique.len(), expected, "{} repeats a pocket", spec.id);
            assert!(spec.numbers.iter().all(|n| *n < POCKETS), "{}", spec.id);
            assert!(
                spec.numbers.windows(2).all(|w| w[0] < w[1]),
                "{} unsorted",
                spec.id
            );
        }
    }

    #[test]
    fn the_zero_is_only_reachable_where_a_real_table_allows_it() {
        // The one pocket that decides the house edge: if an outside bet ever
        // covered it, the game would be fair and the house would be broke.
        let touching_zero: HashSet<_> = catalogue()
            .values()
            .filter(|spec| spec.covers(0))
            .map(|spec| spec.id.as_str())
            .collect();
        let expected: HashSet<_> = [
            "straight:0",
            "split:0-1",
            "split:0-2",
            "split:0-3",
            "trio:0-1-2",
            "trio:0-2-3",
            "basket:0-1-2-3",
        ]
        .into_iter()
        .collect();
        assert_eq!(touching_zero, expected);
    }

    #[test]
    fn every_pocket_is_reachable_and_the_outside_splits_the_wheel_evenly() {
        for number in 0..POCKETS {
            assert!(
                catalogue().values().any(|spec| spec.covers(number)),
                "nothing covers {number}"
            );
        }
        for pair in [("red", "black"), ("odd", "even"), ("low", "high")] {
            let left = bet(pair.0).unwrap();
            let right = bet(pair.1).unwrap();
            assert_eq!(left.numbers.len(), 18);
            assert_eq!(right.numbers.len(), 18);
            assert!(
                left.numbers.iter().all(|n| !right.covers(*n)),
                "{} and {} overlap",
                pair.0,
                pair.1
            );
        }
        // Red is the wheel's own colouring, not the board's.
        assert!(
            bet("red")
                .unwrap()
                .numbers
                .iter()
                .all(|n| colour(*n) == Colour::Red)
        );
        assert!(
            bet("black")
                .unwrap()
                .numbers
                .iter()
                .all(|n| colour(*n) == Colour::Black)
        );
    }

    #[test]
    fn dozens_and_columns_tile_the_board_without_overlapping() {
        for group in ["dozen", "column"] {
            let mut seen = Vec::new();
            for index in 1..=3 {
                seen.extend(
                    bet(&format!("{group}:{index}"))
                        .unwrap()
                        .numbers
                        .iter()
                        .copied(),
                );
            }
            seen.sort_unstable();
            assert_eq!(seen, (1..POCKETS).collect::<Vec<_>>(), "{group}");
        }
        assert_eq!(bet("column:1").unwrap().numbers[0], 1);
        assert_eq!(bet("column:3").unwrap().numbers[0], 3);
        assert_eq!(bet("dozen:2").unwrap().numbers[0], 13);
    }

    #[test]
    fn every_bet_carries_the_same_house_edge() {
        // A single-zero wheel pays as if there were 36 pockets and spins 37, so
        // every bet on the felt loses 1/37 of its stake in expectation. Any
        // shape whose odds were entered by hand rather than derived would show
        // up here as a different number.
        for spec in catalogue().values() {
            let wins = spec.numbers.len() as f64;
            let stake = 1.0;
            let expected =
                (wins / f64::from(POCKETS)) * (spec.kind.payout() as f64 + 1.0) * stake - stake;
            assert!(
                (expected - (-1.0 / f64::from(POCKETS))).abs() < 1e-12,
                "{} returns {expected}",
                spec.id
            );
        }
    }

    #[test]
    fn a_winning_stake_comes_back_with_the_stake_itself() {
        assert_eq!(bet("straight:17").unwrap().returns(100), 3600);
        assert_eq!(bet("split:17-20").unwrap().returns(100), 1800);
        assert_eq!(bet("corner:17-18-20-21").unwrap().returns(100), 900);
        assert_eq!(bet("red").unwrap().returns(100), 200);
        assert_eq!(bet("dozen:2").unwrap().returns(100), 300);
    }

    #[test]
    fn a_bet_the_geometry_cannot_make_is_not_in_the_catalogue() {
        // Numbers that do not touch on the felt, a straight-up off the wheel,
        // and ids in the wrong order or shape.
        for id in [
            "split:1-5",
            "split:3-4",
            "straight:37",
            "corner:1-2-3-4",
            "split:20-17",
            "street:1-2",
            "dozen:4",
            "column:0",
            "green",
            "",
        ] {
            assert!(bet(id).is_none(), "{id} should not be a legal bet");
        }
        // But the ones that look similar and *are* legal still are.
        assert!(bet("split:1-4").is_some(), "1 and 4 touch down the column");
        assert!(bet("split:2-3").is_some(), "2 and 3 touch across the row");
    }
}

#[cfg(test)]
mod table_tests {
    use super::*;
    use chrono::Utc;

    fn seated() -> RouletteTable {
        let mut table = RouletteTable::new(uuid::Uuid::from_u128(7));
        table.stack = BUY_IN;
        table
    }

    #[test]
    fn chips_on_the_felt_are_a_claim_on_the_stack_not_a_withdrawal() {
        let mut table = seated();
        table.place("straight:17", 500).unwrap();
        table.place("red", 2_500).unwrap();
        // Nothing has been lost yet, so the table is still worth its stack --
        // which is what lets the conservation check count one number (§V1).
        assert_eq!(table.stack, BUY_IN);
        assert_eq!(table.staked(), 3_000);
        assert_eq!(table.available(), BUY_IN - 3_000);
    }

    #[test]
    fn a_spin_moves_exactly_what_was_staked_and_what_was_won() {
        let mut table = seated();
        table.place("straight:17", 500).unwrap();
        table.place("red", 2_500).unwrap();
        // 17 is black, so the straight-up pays 35 to 1 and the red loses.
        let spin = table.spin(17, 1, Utc::now()).unwrap();
        assert_eq!(spin.staked, 3_000);
        assert_eq!(spin.returned, 500 * 36);
        assert_eq!(spin.net(), 18_000 - 3_000);
        assert_eq!(table.stack, BUY_IN - 3_000 + 18_000);
        assert!(
            table.chips.is_empty(),
            "the felt is cleared for the next spin"
        );
        assert_eq!(table.history, vec![17]);
    }

    #[test]
    fn a_spin_that_misses_everything_costs_exactly_the_stake() {
        let mut table = seated();
        table.place("straight:17", 500).unwrap();
        table.place("dozen:1", 500).unwrap();
        let spin = table.spin(0, 1, Utc::now()).unwrap();
        assert_eq!(spin.returned, 0);
        assert!(spin.wins.is_empty());
        assert_eq!(table.stack, BUY_IN - 1_000);
    }

    #[test]
    fn one_pocket_can_win_several_bets_at_once() {
        let mut table = seated();
        for id in [
            "straight:17",
            "split:17-20",
            "corner:17-18-20-21",
            "black",
            "dozen:2",
        ] {
            table.place(id, 100).unwrap();
        }
        table.place("red", 100).unwrap();
        let spin = table.spin(17, 1, Utc::now()).unwrap();
        // 17: black, second dozen, and every inside shape that touches it.
        assert_eq!(spin.wins.len(), 5);
        let expected = 100 * 36 + 100 * 18 + 100 * 9 + 100 * 2 + 100 * 3;
        assert_eq!(spin.returned, expected);
        assert_eq!(table.stack, BUY_IN - 600 + expected);
    }

    #[test]
    fn chips_on_one_spot_stack_up_and_are_paid_as_one() {
        let mut table = seated();
        table.place("red", 2_500).unwrap();
        table.place("red", 2_500).unwrap();
        assert_eq!(table.on_spot("red"), 5_000);
        let spin = table.spin(3, 1, Utc::now()).unwrap();
        assert_eq!(
            spin.wins.len(),
            1,
            "one spot is one win, however many chips"
        );
        assert_eq!(spin.wins[0].staked, 5_000);
        assert_eq!(spin.wins[0].returned, 10_000);
    }

    #[test]
    fn the_felt_refuses_what_a_table_would_refuse() {
        let mut table = seated();
        assert_eq!(
            table.place("split:1-5", 100),
            Err(RouletteError::UnknownBet)
        );
        assert_eq!(table.place("red", 250), Err(RouletteError::NotAChip));
        assert_eq!(table.place("red", 0), Err(RouletteError::NotAChip));
        for _ in 0..2 {
            table.place("red", 10_000).unwrap();
        }
        assert_eq!(table.on_spot("red"), MAX_SPOT);
        assert_eq!(table.place("red", 100), Err(RouletteError::SpotLimit));
        assert!(table.chips.len() == 2, "a refused chip never lands");
    }

    #[test]
    fn you_cannot_bet_chips_you_have_not_got() {
        let mut table = RouletteTable::new(uuid::Uuid::from_u128(7));
        table.stack = 1_000;
        table.place("red", 500).unwrap();
        table.place("black", 500).unwrap();
        assert_eq!(table.available(), 0);
        assert_eq!(table.place("odd", 100), Err(RouletteError::Broke));
        // And a stack that is entirely on the felt still spins.
        let spin = table.spin(5, 1, Utc::now()).unwrap();
        assert_eq!(spin.staked, 1_000);
        assert_eq!(table.stack, 1_000);
    }

    #[test]
    fn the_felt_holds_a_bounded_number_of_chips() {
        let mut table = seated();
        for _ in 0..MAX_CHIPS {
            table.place("red", 100).unwrap();
        }
        assert_eq!(table.place("black", 100), Err(RouletteError::Crowded));
    }

    #[test]
    fn lifting_a_chip_puts_it_back_in_the_stack() {
        let mut table = seated();
        table.place("straight:7", 500).unwrap();
        table.place("straight:8", 100).unwrap();
        let lifted = table.undo().unwrap();
        assert_eq!(lifted.bet, "straight:8");
        assert_eq!(table.staked(), 500);
        assert_eq!(table.clear(), 500);
        assert_eq!(table.available(), BUY_IN);
        assert!(table.undo().is_none());
    }

    #[test]
    fn a_rebet_puts_the_same_layout_back_or_none_of_it() {
        let mut table = seated();
        table.place("straight:17", 500).unwrap();
        table.place("red", 2_500).unwrap();
        table.spin(0, 1, Utc::now()).unwrap();
        table.rebet().unwrap();
        assert_eq!(table.staked(), 3_000);
        assert_eq!(table.on_spot("straight:17"), 500);

        // With too little left, the whole layout is refused and the felt is
        // left exactly as it was rather than half-covered.
        table.clear();
        table.spin(0, 1, Utc::now()).unwrap_err();
        table.stack = 1_000;
        table.place("black", 500).unwrap();
        assert_eq!(table.rebet(), Err(RouletteError::Broke));
        assert_eq!(table.staked(), 500);
        assert_eq!(table.on_spot("black"), 500);
    }

    #[test]
    fn spinning_an_empty_felt_is_refused() {
        let mut table = seated();
        assert_eq!(table.spin(0, 1, Utc::now()), Err(RouletteError::NoBets));
        assert_eq!(table.spins, 0);
    }

    #[test]
    fn the_marquee_keeps_the_recent_numbers_and_no_more() {
        let mut table = seated();
        for number in 0..(HISTORY as u8 + 5) {
            table.place("red", 100).unwrap();
            table.spin(number % POCKETS, 1, Utc::now()).unwrap();
        }
        assert_eq!(table.history.len(), HISTORY);
        assert_eq!(table.history[0], HISTORY as u8 + 4, "newest first");
        assert_eq!(table.spins, HISTORY as u64 + 5);
    }

    #[test]
    fn the_house_keeps_its_edge_over_a_long_night() {
        // Every pocket, backed on every shape, is the whole wheel played out:
        // the table must come out one pocket ahead on each, which is the 2.70%
        // the payouts encode. A single mispriced shape breaks this.
        for spec in catalogue().values() {
            let mut table = RouletteTable::new(uuid::Uuid::from_u128(1));
            table.stack = 10_000_000;
            let opening = table.stack;
            for number in 0..POCKETS {
                table.place(&spec.id, 100).unwrap();
                table.spin(number, 1, Utc::now()).unwrap();
            }
            let staked = 100 * i64::from(POCKETS);
            let paid = spec.numbers.len() as i64 * spec.returns(100);
            assert_eq!(table.stack - opening, paid - staked, "{}", spec.id);
            assert_eq!(table.stack - opening, -100, "{} is mispriced", spec.id);
        }
    }
}
