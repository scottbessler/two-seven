# two-seven — Spec (living document)

A multiplayer poker web app. First (and currently only) variant: **Texas Hold'em**.
Same stack and conventions as [screwball](https://github.com/scottbessler/screwball):
Rust + Axum server-side rendering, passkey auth, SSE for live updates, a single
no-build Preact/htm island for interactivity, JSON files on a Fly volume for
persistence.

This file is the source of truth for design decisions, invariants and
outstanding work. Keep it up to date with every change.

---

## 1. Goals / non-goals

Goals

- Real multiplayer Hold'em: several humans at one table, live updates, no page reloads.
- Bots with visibly different playing styles and difficulty levels, so a table is
  always playable solo.
- Cash tables in two flavours: **limit** and **no-limit** (see §4 Bank).
- Tournaments (single table first).
- A persistent **bank**: every player (and every bot type) has an account.
  Accounts never go negative; users may re-up $1,000 only while below $1,000,
  repay loans from the coin menu, and see their current loan count.
- Variant-agnostic plumbing: tables/bank/bots/tournaments should not assume
  Hold'em, so Omaha / 2-7 triple draw can be added later.

Non-goals (for now)

- Real money of any kind. Balances are play money, denominated in cents.
- Multi-table tournaments, rake, chat, hand-history export, mobile app.
- Anti-collusion / anti-bot detection.

## 2. Tech stack

| Concern | Choice |
| --- | --- |
| Language | Rust, edition 2024 (needs rustc >= 1.85) |
| HTTP | `axum` 0.8 + `tower-http` (`ServeDir`, `TraceLayer`) |
| Auth | `webauthn-rs` passkeys, signed-cookie session (`axum-extra`) |
| Live updates | Server-Sent Events (`tokio::sync::broadcast` fan-out) |
| Rendering | Hand-rolled HTML strings (`src/render.rs`) + escaping helper |
| Interactivity | One Preact/htm island from vendored `public/vendor/htm-preact.js`, no build step |
| Persistence | Pretty JSON files under `DATA_PATH`, atomic temp-write + rename |
| RNG | `rand` with `StdRng`; every shuffle is seeded and recorded for replay |
| Lint | `cargo fmt`, `clippy` (`all` denied, warnings denied, `unsafe` forbidden), `oxlint` for JS |
| Deploy | Docker (bookworm builder + runtime) → Fly.io, volume mounted at `/data` |

Environment variables: `PORT` (8080), `DATA_PATH` (`data`), `RP_ID`,
`RP_ORIGIN`, `SESSION_SECRET`, `PASSKEY_DISABLED` (local/e2e only), `RUST_LOG`.

## 3. Money

- Unit is **cents** (`type Cents = i64`). Never floats. Rendered as `$1,234.56`.
- Chips at a cash table are the same unit as bank money: buying in for $20 moves
  2000 cents from the bank account to the seat stack.
- Tournament chips are **not** money: a tournament seat has a chip stack that is
  unrelated to the bank; only the buy-in and the prizes touch the bank.
- ∀ positive user-configured stake, blind, ante, buy-in, entry fee, or wager ≥ $1.00.
  Zero ante remains valid.

## 4. Bank

```
Account { owner: AccountOwner, balance: Cents, loan_count: u64, entries: Vec<LedgerEntry>, created_at, updated_at }
AccountOwner = User(Uuid) | Bot(BotKind)     // one shared account per bot kind
LedgerEntry  { id, at, kind, delta: Cents, balance_after: Cents, memo }
LedgerKind   = ReUp | HouseStake | LoanRepayment | LoanInterest | BuyIn{table} | CashOut{table} | TournamentBuyIn{tournament}
             | TournamentPrize{tournament} | Adjustment
```

Rules

- A new user's account starts at **$0**. Bot accounts are created lazily, also at $0.
- Every debit must leave the account balance ≥ $0; each gameplay buy-in, entry,
  or wager ≤ $10,000.
- A signed-in user may re-up $1,000 when their balance is < $1,000. Each re-up
  appends a `ReUp` ledger entry and increments `loan_count`.
- A bot's first shortfall-funded buy-in appends a `HouseStake` entry rounded up
  to whole $1,000 loan units without increasing `loan_count`. Later bot
  shortfalls append `ReUp` entries and increment `loan_count` normally.
- Each loan is exactly $1,000, so debt is `loan_count * $1,000`. The coin menu
  can repay one loan when the balance covers $1,000; repayment appends a
  `LoanRepayment` entry and reduces `loan_count` by one. The player page can
  clear every loan the balance covers in one press, as a single
  `LoanRepayment` entry.
- A person only borrows their way into a seat costing $1,000 or less; above
  that the buy-in must be covered by the balance. Bots are staked at every rung
  so the whole ladder stays fillable.
- A user's poker cash-out with winnings charges 1% of those winnings per loan,
  capped at 10 loans; the rounded-down fee is a separate `LoanInterest` entry.
  Bot cash-outs do not pay loan interest.
- Bot buy-ins auto re-up as needed so cash tables remain fillable.
- The admin page can forgive all bot loans at once. Forgiveness clears only
  bot `loan_count` values, leaves balances unchanged, and reports the loans and
  house players affected; human accounts are untouched.
- Legacy bank account JSON is wiped once on the non-debt bank migration.
- Cash-out returns the seat's remaining stack to the account.
- Players can hand each other money in whole $1,000 chips, up to $1,000,000 per
  gift. A gift appends a `Gift` entry to both accounts under one lock — a debit
  naming the recipient, a credit naming the sender — so the total on the books
  does not move. Gifts to yourself, amounts off the $1,000 increment, and gifts
  larger than your balance are refused.
- The bank is the settlement layer: chips only enter play through a `BuyIn` and
  only leave through a `CashOut`/prize, so `sum(balances) + sum(chips in play)`
  is invariant (§V1).
- Every account's `balance` must equal the sum of its ledger deltas (§V2).

Admin controls: the password-protected admin page can reset all money and
loans, reset poker or Blitz stats, and forgive all bot loans without changing
balances or human accounts. Forgiveness reports the number of loans cleared
and house players affected.

UI: the header shows the signed-in user's balance next to their username, with a
coin icon; hovering/tapping it opens a small panel with the current balance,
outstanding debt, net balance, loan count badge, re-up action, repayment action
for the newest loan when affordable, and the most recent ledger deltas. Your own
player page carries a loans panel: what you owe, and one action that pays off
every loan the balance covers. The
panel closes when clicking or tapping outside it or pressing Escape, returning
focus to the coin-menu summary. Seat labels at a table show the seat owner's
bank balance the same way (bots included).
The signed-in player page shows account summary, recent ledger rows, and a
ledger-derived finances-over-time chart. A person's name at a table and in the
standings links to their own copy of that page; the house's does not, having no
page. Somebody else's page adds a $1,000 stepper that sends them money from
your account, and redraws both the summary and the ledger once it lands.

## 5. Hold'em rules implemented

- 2–9 seats. Button rotates clockwise each hand; heads-up uses the standard
  button-posts-small-blind rule.
- Streets: preflop, flop (3), turn (1), river (1); one burn card is *not*
  modelled (irrelevant with a shuffled deck).
- Actions: `fold`, `check`, `call`, `bet`, `raise`, plus implicit all-in when a
  player cannot cover.
- **Blackjack:** Four fixed shared tables with max bets $100 / $1,000 /
  $10,000 / $100,000; the buy-in is 10× the max bet and comes out of the bank
  as one `BlackjackBuyIn` ledger row (cash-out returns the seat stack as one
  `BlackjackCashOut`). Each table offers exactly four wagers — ¼, ½, ¾ and
  the max bet — and up to five human seats; a user holds at most one blackjack
  seat at a time. The shoe and dealer hand are shared: everyone who has bet
  when the round starts is dealt in from the same shoe against the same dealer.
  A lone seated player is dealt as soon as they bet; with two or more seated
  players the first bet starts a betting clock, and seats that have not bet when
  it expires sit that round out. Insurance and hand actions run on the poker
  turn clock; a timed-out insurance decision declines and a timed-out hand
  stands. Double and split require another bet of the active hand to remain
  in the seat stack; insurance requires half that bet. The same affordability
  rules govern displayed action flags and server validation. Results stay on
  the table for a short pause before the next betting round. Seated players can
  Add chips (another buy-in) or Leave; with a live bet both wait for settlement
  and the seat visibly remains Leaving until then.
- **Limit** stakes (`small_bet`/`big_bet`): blinds are `small_bet/2` and
  `small_bet`; bets are `small_bet` preflop and on the flop, `big_bet` on turn
  and river; at most 4 wagers per street (bet + 3 raises).
- **No-limit** stakes (`small_blind`/`big_blind`): min-raise is the size of the
  previous bet/raise, max is the player's stack.
- Side pots: contributions are tracked per seat, and pots are formed by
  ascending all-in levels; each pot is awarded to the best hand among its
  eligible seats, split evenly on ties with odd chips going to the first seat
  left of the button.
- A hand ends early when all but one player folds (no cards shown).
- Showdown reveals the hole cards of every seat still in the hand, in order.

Hand evaluation (`src/eval.rs`): best five of seven cards, categories
high-card < pair < two-pair < trips < straight < flush < full-house < quads <
straight-flush, with wheel (`A2345`) straights. `HandRank` is `Ord`, so ties are
exact equality.

## 6. Tables

```
Table { id, name, variant: Variant::Holdem, stakes: Stakes, mode: TableMode,
        max_seats, min_buy_in, max_buy_in, seats: Vec<Seat>, button, hand: Option<Hand>,
        last_hand: Option<HandSummary>, hand_no, next_action_at, turn_clock: Option<TurnClock>,
        created_at, updated_at }
TurnClock { seat, hand_no, decision, deadline }
Stakes    = Limit { small_bet, big_bet } | NoLimit { small_blind, big_blind }
TableMode = Cash { no_debt: bool } | Tournament(TournamentState)
Seat      { occupant: Empty | Human{user_id} | Bot{kind}, stack, sitting_out, ... }
```

- A hand starts automatically once two or more seats have a positive stack and
  are not sitting out.
- After a hand resolves, the result is kept in `last_hand` and the next deal is
  scheduled a few seconds later (`next_action_at`) so players can read the
  showdown.
- Bots act on a timer as well, giving a human-paced feel; the driver task is the
  only thing that advances bot turns (§8).
- **The turn clock.** A table with more than one person dealt in gives whoever
  is to act ten seconds (`TURN_SECONDS`). When it runs out the table plays the
  cheapest legal action for them: a check where checking is free, a fold where
  it is not. One person alone with the house keeps nobody waiting and is never
  put on the clock, and the house is never on it either — bots act on the
  driver's own pacing. A `TurnClock` belongs to exactly one decision (seat,
  hand, and events logged so far), so time is never carried from one turn to
  the next, and a clock left over from a decision that is finished — or from a
  table the last of the company has left — is simply taken away.

## 7. Bots

One shared bank account per bot kind. Difficulty ladder:

| Kind | Style |
| --- | --- |
| `Fish` | Near-random: calls far too much, raises at random, never folds a pair. |
| `Rock` | Tight-passive: fixed preflop opening ranges, calls with made hands, folds otherwise. |
| `Grinder` | Tight-aggressive: hand strength buckets + pot odds, bets/raises with strong made hands and draws, folds marginal spots. |
| `Shark` | Parameterized position- and stack-aware Monte Carlo policy with action-weighted ranges, draw-aware semi-bluffs, implied-odds calls, intent-based sizing, and opponent-read adjustments; commits short or near-all-in stacks rather than leaving dust. |

Each kind has five regulars with their own name and bankroll, except sharks,
who have nine -- enough to fill the largest table (a nine-seat tournament) on
their own. The nine sharks run one policy with nine tunings: a three-by-three
grid of looseness (opening and defending thresholds) against aggression (bet
edges, bluff frequencies, and value sizing), so no two of them play the same
(`SharkParams::for_regular`). Regular 0 is the reference build, `DEFAULT`.

Who the house will sit follows from the stakes, at a cash table and at a
tournament alike (§V62): below $100,000 anyone, from $100,000 up no fish, from
$500,000 up sharks only. The cash-ladder mix runs 60/20/10/10 fish/grinder/rock/
shark at the cheapest rung and slides evenly from there: the fish are gone by
the $50,000 rung, and the top two rungs are sharks alone.

Bots see only what a player in that seat legitimately sees (their own hole cards
and the board) — the same redacted view a human gets (§V3).

## 8. Real-time and the driver

- `TableStore` broadcasts the id of any table that changed; `GET /tables/{id}/events`
  streams a redacted `TableView` snapshot immediately and on every change.
- A single background task ticks a few times per second and, for each table,
  performs whatever the clock says is due: put the person to act on the turn
  clock or act for them once it has run out (§6), act for a bot whose turn it
  is, or deal the next hand. All mutation goes through the same engine entry
  points the HTTP handlers use, so there is one rules path.
- The `TableView` carries `turn_deadline` and `turn_seconds` so the client can
  draw the countdown without knowing the rule; a table nobody is on the clock
  at sends no deadline and draws nothing.

## 9. Routes

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/` | Lobby: bank widget, open tables, tournaments, your seats |
| GET | `/player` | Signed-in player's account, options and finance history |
| POST | `/player/settings` | Save your own account options: `{unfunded_tournaments, see_bot_cards}` |
| GET | `/healthcheck` | Liveness for Fly |
| POST | `/auth/register/begin`, `/auth/register/finish` | Passkey registration |
| POST | `/auth/login/begin`, `/auth/login/finish` | Passkey login |
| POST | `/auth/logout` | Clear session |
| GET | `/tables/new` | Create-table form |
| POST | `/tables` | Create a table |
| GET | `/tables/{id}` | SSR table page + island mount |
| GET | `/tables/{id}/state` | Redacted `TableView` JSON (SSE fallback) |
| GET | `/tables/{id}/events` | SSE stream of `TableView` |
| POST | `/tables/{id}/join` | Buy in to the first open seat: `{}` (bank-checked) |
| POST | `/tables/{id}/leave` | Stand up and cash out |
| POST | `/tables/{id}/rebuy` | Top up the seat stack from the bank |
| POST | `/tables/{id}/bot` | Seat/remove a bot: `{seat, kind?}` |
| POST | `/tables/{id}/action` | `{kind: fold\|check\|call\|bet\|raise, amount?}` |
| POST | `/tables/{id}/emote` | Seated human emits `{kind: cry\|joy\|laugh\|poop\|shock}` |
| GET | `/tournaments/new`, POST `/tournaments` | Create a sit-and-go |
| POST | `/tournaments/{id}/register` | Buy in to the first open seat: `{}` |
| GET | `/api/bank` | Balance, derived debt/net/next repayment + recent ledger entries |
| POST | `/api/bank/repay` | Repay the newest outstanding loan principal |
| POST | `/api/bank/repay-all` | Repay every loan the balance covers: `{}` |

HTML routes return an escaped error page (`AppError`); JSON routes return
`{"error": "..."}` with 400/401/404/409/422.

## 10. Tournaments (single table, sit-and-go)

- Creating one requires the buy-in to be a cash-ladder rung, and to be covered
  by your balance unless the `unfunded_tournaments` account option is on.
- Config: buy-in, seat count, starting chips, blind schedule (level = list of
  `{small_blind, big_blind, ante, hands}`), payout percentages.
- Registering charges the buy-in from the bank (respecting `no_debt` if set) and
  seats the player. Bots can fill the remaining seats, subject to the same
  stakes constraint the cash ladder applies (§V62).
- Blinds go up after the configured number of hands per level.
- A player is eliminated at zero chips; finishing position is recorded and
  prizes are paid into the bank when the tournament ends.

## 11. Project layout

```
src/
  main.rs, lib.rs, app.rs        # startup, router, state
  auth.rs, session.rs, users.rs  # passkeys, cookie session, user store
  error.rs                       # AppError -> HTML/JSON
  cards.rs                       # Card/Rank/Suit/Deck (seeded shuffle)
  eval.rs                        # 7-card hand evaluation
  holdem/                        # hand engine statechart (see STATECHART.md)
    mod.rs                       #   shared types, side pots, showdown resolution
    street.rs                    #   hand machine: street progression + fold win
    round.rs                     #   betting round machine: actions + rotation
  table.rs                       # table/seat lifecycle, buy-ins, hand plumbing
  money.rs                       # cents type and money formatting
  tournament.rs                  # sit-and-go state
  bank.rs                        # accounts + ledger
  bot.rs                         # bot policies
  driver.rs                      # background ticker (bot turns, next deal)
  store.rs                       # TableStore persistence and broadcasts
  render.rs                      # SSR
  routes.rs                      # handlers
  view.rs                        # redacted view projections
public/                          # table.js island, auth.js, vendor/
tests/                           # eval, holdem, bank, bot, routes, dockerfile
```

## 12. Milestones

1. Scaffolding: Cargo, CI, Docker, Fly, hooks, auth/users/session, lobby shell. **Done.**
2. Cards, evaluator, betting engine with side pots + unit tests. **Done.**
3. Bank + tables: create/join/leave/rebuy, no-debt enforcement, settlement. **Done in the initial HTTP/store implementation; crash recovery and driver scheduling remain outstanding.**
4. Bots + driver. **Done: deterministic policy bots, bankroll seating, and ticking driver.**
5. Table UI: SSR page, SSE, island, bank widget. **Done: responsive island, lobby, bank widget, and asset contracts.**
6. Sit-and-go tournaments. **Done: registration, scheduled antes/blinds, bot fill, elimination, payout, and tournament views.**
7. Polish: e2e snapshots, mobile layout. **Done: lobby, money-entry, table layout, showdown, tournament and bank-widget UX, browser checks, and regression contracts.**

Mark each milestone done here as it lands.

## 13. Outstanding tasks

- [ ] Action clock / auto-fold for idle humans (today a table waits forever).
- [ ] Multi-table tournaments, late registration, rebuy periods.
- [ ] More variants: Omaha, and 2-7 triple draw (the repo's namesake).
- [ ] Push notifications for "it's your turn" (screwball has the VAPID plumbing).
- [ ] Hand history browser + replay from the recorded shuffle seed.
- [ ] Playwright snapshot coverage for the table page.
- [ ] Browser-driven regression snapshots for lobby, showdown, bank panel, and tournament layouts.
- [ ] Bank statement page (paginated ledger) beyond the hover panel.
- [ ] Rake / house account, if tables ever need a sink.
- [ ] Tournament crash-recovery reconciliation if a prize ledger write fails mid-payout.
- [ ] Regression coverage for live-hand departure/rebuy conservation and tournament departure recovery.

## §V Verification invariants

- **V1** Chips are conserved: bank balances + all seat stacks + chips in the
  current pot is constant except at explicit buy-in/cash-out/prize boundaries.
- **V2** `account.balance == sum(entry.delta)` and each entry's `balance_after`
  matches the running total.
- **V3** A `TableView` never contains another seat's hole cards before showdown,
  nor any undealt card; bots consume the same projection. Sole exception: the
  V60 bot x-ray, which no seated human at all — viewer included — can be
  present for.
- **V4** Pot distribution pays out exactly the pot: the sum of awards equals the
  sum of contributions, for any all-in/side-pot configuration.
- **V5** Bank accounts never go below zero; user re-up is only allowed below
  $1,000 and increments `loan_count`; a bot's first shortfall-funded buy-in
  uses a non-loan `HouseStake`; repayment requires $1,000 and decrements
  `loan_count` by one.
- **V6** Every reachable hand state has at least one legal action for the player
  on turn, and the engine rejects any action not in that set.
- **V7** The shared card face renders all 52 cards with bold corner ranks,
  undecorated centered aces, and unhatched chess-piece portraits for J/Q/K in
  both SSR and island views; desktop and mobile snapshots cover the full deck.
- **V8** Every `/card-test` suit row keeps the in-game card dimensions while
  wrapping all 13 cards within its visible width; no suit row scrolls horizontally.
- **V9** ∀ positive configured stake, blind, ante, buy-in, entry fee, or wager ≥ 100 cents.
- **V10** ∀ single gameplay buy-in, entry, rebuy, or wager ≤ 1,000,000 cents;
  a buy-in auto-loan adds one `loan_count` per required $1,000 loan, except
  the first shortfall-funded bot buy-in uses `HouseStake` instead. A person's
  buy-in only auto-loans at a seat costing ≤ $1,000; above that an uncovered
  buy-in is refused, and a table is only offered as affordable when the balance
  covers it or it lends. Bot buy-ins auto-loan at every buy-in.
- **V16** Every loan is exactly $1,000; debt equals `loan_count * $1,000`,
  repayment costs $1,000, decrements `loan_count` by one, and cannot make an
  account balance negative. Paying off every affordable loan at once costs
  $1,000 per loan cleared, clears `min(loan_count, balance / $1,000)` of them,
  and is one ledger entry.
- **V17** A user's poker cash-out charges at most 10% of positive table winnings
  for loan interest, rounded down and recorded separately; no fee is charged
  without winnings or loans, and bots never pay this fee.
- **V58** A gift moves money between two accounts and never makes any: both
  ledger entries are written under one lock, the amount is a whole $1,000 chip
  up to $1,000,000, and a gift to yourself, off the increment, or beyond your
  balance leaves both accounts untouched.
- **V37** Admin bot-loan forgiveness clears `loan_count` on every indebted bot
  without changing balances or human accounts, and persists the affected
  accounts.
- **V11** `TableView` exposes redacted action events + per-seat hand state; UI shows
  current actor, dealer, blinds, street wager, folded/all-in state, and recent log.
- **V12** Every cash table has one fixed buy-in; human joins, bot seats, and rebuys
  charge and assign exactly that server-configured amount.
- **V13** Every bot personality produces at least one bet or raise across the
  deterministic aggression corpus while retaining distinct style signals.
- **V14** Desktop + mobile table snapshots keep occupied players outside felt,
  viewer cards at viewer seat, board unobscured, actions before unified log,
  one-line header, whole-dollar labels, and integrated showdown winners/cards.
- **V15** Viewer cards retain face saturation inside seat, expose persisted
  50–200% relative size control, magnify as a whole hand on hover/focus where a
  pointer hovers and on hold where none does, and sit on compact rounded-rect felt.
- **V16** Viewer name, stack and street wager read down one left-aligned column
  beside the viewer's hole cards at desktop and mobile widths, never underneath
  them.
- **V17** Card display config persists 50–200% relative viewer size/rank
  size/weight controls; 100% equals former maxima (180%/150%/900), either viewer
  card magnifies both, and default geometry clears board/status.
- **V18** Showdowns remain visible for 6 seconds and fold-only results for 3
  seconds unless a seated human acknowledges early; UI shows OK + deadline progress.
- **V19** Card config opens from page upper-right and previews the exact viewer
  dimensions/font; seat ranks retain suit color, revealed hands are legible, and
  winners have unmistakable gold emphasis without wager/card artifacts.
- **V20** Every occupied-seat player tooltip remains fully inside the viewport at
  desktop and mobile widths, including seats on the top player rail.
- **V21** Rank size scales both rank and suit glyphs; at 200% card/rank settings,
  viewer cards/wager clear table center content and corner indices/pips remain distinct.
- **V22** The table log has a fixed responsive height; added events scroll inside
  it without moving controls or content below the log.
- **V23** Human table lifecycle exposes exactly one state-dependent command:
  unseated players can Buy In to the server-selected first open seat, busted cash
  players can Re-Buy In, seated players can Leave, and deferred live-hand leaves
  visibly remain Leaving until settlement; no human seat picker or sit-out command.
- **V24** Blackjack validates action legality and additional wager bounds under
  one game lock before mutation; rejected actions move no chips or ledger rows.
- **V25** Blackjack view action flags and store validation use the same
  predicates; server-derived wagers never trust client state.
- **V26** Blackjack peeks at deal time unless an ace-up hand has a real
  insurance decision; ace-up decisions peek immediately after insurance or any
  other action, player/dealer naturals push, and insurance pays 3× its stake.
- **V27** Each user holds at most one blackjack seat across the four tables;
  a live bet defers leave/rebuy until settlement, and a seated player's stack
  is the only money a table can win or lose for them.
- **V63** Blackjack tables are shared: one shoe and one dealer hand per round,
  every seat that bet is dealt from it, the betting clock exists only with two
  or more seated players, unbet seats sit the round out, and betting/insurance/
  action deadlines are enforced by the driver (decline insurance, stand).
- **V28** Blackjack and Hand Blitz islands render only legal controls and show
  server error text; shared island helpers remain behavior-compatible.
- **V24** Blackjack tables (seats, stacks, shoe) survive process restart
  through atomic JSON persistence; an interrupted round is not restored — live
  bets are refunded to the seat stack and the table reopens for betting.
- **V25** Hand Blitz runs expire server-side after their round deadline (with a
  small request grace), each user has at most one live run, and completed runs
  are eventually pruned; starts charge only after successful creation.
- **V29** Each new Hold'em hand stores its replay seed, but the seed is mixed
  from fresh entropy plus table id and hand number; it is not equal to, nor
  predictable from, the table's sequential `hand_no`.
- **V30** Idle driver ticks do not persist or broadcast table state; SSE table
  events are emitted only for actual table mutations or explicit keep-alives.
- **V31** Bank-affecting island actions publish the refreshed account state;
  blackjack controls and the header balance update without page refresh.
- **V32** Bots never fold when `Check` is legal; free continuation is chosen over
  surrender for every app bot policy.
- **V33** No field of the live table view exposes a hand's outcome before that
  outcome exists on the board. While a hand is live — runout included — seat
  identity is fixed: `viewer_seat` and `viewer_eliminated` answer who was dealt
  in, not who currently holds chips. Terminal tournament winner state stays
  embargoed until the settled result pause has finished.
- **V34** Table preferences persist opt-in 1-second holds for Fold and All In;
  when enabled, Fold and any action committing actor's remaining stack submit
  only after uninterrupted button hold, without changing submitted action kind.
- **V35** The player page finance chart is derived from ledger entry
  `balance_after` values and shows the current signed-in player's account only.
- **V36** All-in showdown summaries expose per-seat equity at reveal and each
  runout street; non-leading players with 1-9 immediate outs expose those cards.
- **V37** All-in showdown odds expose one box per player until the result is
  final: stacked in the reserved right rail on phones (V64 places them in a
  full-width row instead), one compact horizontal row on wide screens. Adding
  odds never wraps center content into the viewer card area.
- **V38** Blackjack trainer settings travel with each dealt hand: 1/2/8-deck
  shoe choice, percentage-of-shoe cut-card penetration defaulting to 50%,
  visible-card Hi-Lo tutor/log, post-hand running-count quiz, and server-side
  basic-strategy analyzer feedback.
- **V39** Blackjack settings use the shared page header gear like poker; the
  game body renders stable rows for status, hands, trainer feedback, and actions
  without an in-table settings control.
- **V40** Blackjack bet analyzer separates optional insurance advice from
  hit/stand/double/split hand strategy; an available insurance decision cannot
  make a hard hand recommendation say Stand.
- **V41** Blackjack keeps a persistent per-user shoe and accumulated Hi-Lo
  count across hands; a frozen percentage cut card or the safe reserve rule
  triggers reshuffles only at hand start, and the active shoe's dealt cards,
  remaining cards, and exact cut-card marker are always visualized.
- **V42** Mobile poker and blackjack game surfaces fit portrait and landscape
  viewports without document or internal stage scrolling; action buttons share
  one height/font and keep labels contained, and blackjack cards remain readable.
  A blackjack hand fits its cards inside its own width however many it holds,
  and every hand on the table — split hands included — gets an equal share of
  the play area. Fitting is ⊥ scroll *and* ⊥ clip: a box whose content exceeds
  it under `overflow:hidden` has not fitted, it has hidden the difference, so
  the test reads `scrollHeight` against `clientHeight` per box rather than
  asking the document whether it scrolls.
- **V43** Completed desktop showdowns reserve stable clearance for outside
  outcome badges; seats, cards, and badges never overlap center table content.
- **V44** Action bars distribute their visible buttons across the full available
  width; visible content renders in its own layer/row and is not obscured by
  neighboring cards, rows, controls, or overlays except explicit hover zooms.
- **V45** Compact seats never clip their own rows: wagers, dealer/blind badges,
  and outcome badges all remain fully visible and non-overlapping at mobile
  viewports. Dealer/blind badges never buy a row of their own — they float in
  the seat corner, sharing the name's row, and the name stays centered with
  enough width withheld to clear them.
- **V46** Interactive controls use one responsive size contract: buttons keep
  shared heights, contained single-line labels, and usable tap targets across
  mobile and narrow desktop surfaces; blackjack actions remain readable within
  the dense action-row contract. Every control on a row answers to that row's
  tier — the emote taps included: a square target at the tier's own control
  height, never a smaller one beside a full-sized neighbour.
- **V47** Poker action bar = Fold edge + ≤5 equal middle actions + right edge;
  every edge action is ≤ 1/7 of the bar. The right edge carries All In and, when
  the wager range allows one, the custom wager beside it (V56);
  stack-consuming calls/wagers render only as right-edge All In.
  A call that closes a capped pot takes the same right edge slot and colour but
  keeps its own `Call <amount>` label and submits without a hold, since the
  caller still has chips behind; Fold keeps its narrow slot and a dead zone
  separates the two.
- **V48** Portrait poker: viewer name + stack + wager share 1 column; Pot + Current
  Bet stack left of shared cards; stage clips ⊥; log absorbs spare height;
  History/Leave stay at viewport bottom. V64 moves the metrics and caps the log
  at 5 opponents; the rest holds at every count.
- **V49** Coin menu owns 1 persistent control per bank action; bank updates mutate
  existing controls, successful mutation closes menu, closed panel paints ⊥.
- **V50** Portrait poker action-band excess ≤ 1rem; non-bottom actions receive
  safe-area padding ⊥; page bottom padding ≤ .25rem + `--safe-bottom` (the
  inset is reserved once, by the page); footer reaches viewport edge.
- **V51** No-limit streets never cap wager count. Fixed-limit cap state may
  remove wager actions but renders no action-bar status row; visible buttons
  remain on one aligned row. Raising needs a live opponent: once every other
  unfolded player is all in, the only legal actions are fold and call — or a
  short all-in when the call already exceeds the actor's stack.
- **V52** Eliminated tournament seats remain for payout attribution but render
  as spectators after reload: lobby active-seat state + table viewer state ⊥.
- **V53** Narrow poker metrics anchor left (V64 anchors them in the seat grid's
  6th cell at 5 opponents) and keep their box between hands. An
  empty board and a seat between hands both reserve a card's space, the viewer's
  own panel reserving both of its cards and a fixed column for name/stack/wager,
  so its box and the hand inside it hold one position whatever the state — live
  hand, no hand, or a result that pays the viewer. Result-state action bar keeps
  its vertical position; protected controls suppress native selection. Bot
  raises ≤3 per betting street.
- **V54** Mobile e2e emulation matches the shipped target: an iPhone with a
  Dynamic Island running the installed PWA (393x852 portrait, 430x932 Max,
  852x393 landscape, 375x667 short portrait), with safe-area insets pinned
  rather than left at Chromium's zero. ∄ mobile layout test on a viewport
  whose insets are zero — a notchless phone nobody owns proves nothing (B9).
  The insets are an app-wide contract, ⊥ a table one: ∀ page, the `.page`
  gutter reserves `--safe-left`/`--safe-right`/`--safe-bottom`, so ∄ control
  behind the Dynamic Island in landscape or under the home indicator, and a
  shell that wants a tighter budget overrides the gutter tokens, ⊥ the rule. Every game surface stays fit-to-viewport across that set:
  viewer and blackjack card heights answer to the height actually available, not
  to a fixed breakpoint. `viewport-fit=cover` ships with `maximum-scale=1` and
  `user-scalable=no`, insets reach layout only through `--safe-*` custom
  properties, and game shells size with `100vh` (never `100dvh` or `100%`).
  Height budgets subtract the insets through `--usable-h`. Snapshots paint the
  device's own chrome — status bar, Dynamic Island, home indicator — so they can
  be read against a real screenshot. The root canvas paints felt behind the
  translucent status bar; standalone target iPhones retain home-indicator
  clearance when WebKit reports a zero bottom inset.
- **V55** Portrait poker centre = pot/current-bet left, shared cards centred in
  what is left, and a reserved right rail. The rail carries the showdown result
  right-aligned and, until the result is final, one equity/outs box per live
  player; it holds its width whether or not anything is in it, and the shared
  cards size to the room that leaves them.
- **V56** Whenever a wager range has more than one legal amount, the action bar
  offers a custom wager to the right of All In. It opens a modal slider over the
  whole legal range in whole-dollar detents, with both the minimum and the exact
  maximum reachable, and confirms with a button naming the street total it
  raises to. It submits the same chips a preset wager would; the presets and the
  hold-to-confirm All In are unchanged.
- **V57** ∀ seat dealt into the hand in progress, its occupant is fixed until
  `table.hand` is `None`. A person buying into an occupied seat is recorded as
  that seat's `pending_arrival`, is charged once, is never a player in the hand
  already dealt, and takes the seat only after settlement — with exactly the
  table buy-in, while the house player leaves with its settled chips. Backing
  out before the swap returns the buy-in.
- **V59** Betting closed with ≥2 live players & board incomplete is a real
  persisted hand state, ⊥ a client animation: `table.hand` stays `Some` and the
  board grows one street per explicit advance. ∀ street dealt during runout, one
  `Advance` deals exactly one street — a seated human's press, or the
  always-armed `RUNOUT_STEP_SECONDS` deadline, whichever lands first; a press
  before `RUNOUT_FLOOR_MS` is refused so no one can skip the table's look at the
  card. Hole cards of every unfolded seat expose the moment betting closes.
  Pots are awarded & the hand settles only once the board is complete ∴ ⊥ result
  exists to embargo, and ⊥ seat is eliminated before the last card is face up.
- **V60** Account options live in `User.settings` (server-held ∵ server enforces
  them), edited only by their owner via `POST /player/settings`.
  `unfunded_tournaments` drops the balance check on *creating* a tournament
  only — the ladder-rung check & every buy-in charge stand. `see_bot_cards`
  exposes bot seats' hole cards ⟺ ∄ seated human, viewer included; any human
  sitting down ⇒ face down again for everyone.
- **V62** The buy-in decides which house players may be seated, at a cash table
  and at a tournament alike: no `Fish` from $100,000 up, nothing but `Shark`
  from $500,000 up. The standing tables' mix and every seat they fill obey it,
  a bot seating request that breaks it is refused, and no two of the nine
  sharks play with the same tuning. At startup every standing table is
  reconciled against the lineup its rung calls for, seat by seat: a house
  player the seat does not call for stands up and the tick refills it, so a
  table saved under an older mix converges on the current one instead of
  keeping a lineup nothing would seat today.
- **V61** Other-player seats → compact dark tiles across desktop/tablet/portrait/
  landscape; stable identity+role, stack, cards/state, wager rows; existing
  acting/leading/winner/champion/folded/all-in semantics remain distinct;
  content ⊥ clip/overlap; viewer seat unchanged.
- **V63** A seated human may emit only `cry|joy|laugh|poop|shock`. Every
  accepted tap produces one ephemeral SSE event with a unique id, seat, and
  kind; ⊥ persistence/game-state mutation. Clients animate every event upward
  from that seat, including rapid repeats. Spectators and bots cannot emit.
- **V64** Portrait phone, 5 opponents: seats regrid to 3 columns, so 3 + 2 tiles
  fill both rows and Pot + Current Bet take the 6th cell. No cell is empty. The
  board then owns the full stage width (no side rails), all-in odds become one
  full-width row beneath it, and the log caps at 4 lines so the surplus reaches
  the board and the tiles. Narrows V37, V48, V53 for this count only; every
  other count keeps them as written. Log ⊥ growth with events (V22) — it fills
  to its cap, so the footer stays at the viewport bottom with no empty band.

## §T Build tasks

id|status|task|cites
T1|x|enforce $1 floor + $10k per-entry ceiling|V1,V2,V5,V9,V10
T2|x|replace setup forms with 3 cash + 3 tournament presets|V9
T3|x|add structured hand events + per-seat hand state|V3,V4,V6,V11
T4|x|render game log + table cues|V7,V11
T5|x|replace cash buy-in ranges with one fixed amount|V1,V2,V9,V10,V12
T6|x|restore and regression-test bot aggression|V6,V13
T7|x|recompose live table + showdown UI|V3,V7,V11,V14
T8|x|tune viewer cards + compact felt geometry|V7,V14,V15
T9|x|place viewer wager above hole cards|V14,V16
T10|x|add card display config + paired hand magnification|V14,V15,V17
T11|x|hold showdown for acknowledgement + countdown|V11,V14,V18
T12|x|polish card config + showdown card emphasis|V14,V17,V19
T13|x|rebase card controls around former maxima|V15,V17,V19
T14|x|contain player tooltips at viewport edges|V14,V20
T15|x|reflow max-size viewer cards and card faces|V17,V19,V21
T16|x|reserve a fixed table-log footprint|V11,V14,V22
T17|x|simplify and harden table lifecycle controls|V2,V10,V23
T18|x|apply atomic legality, wager bounds, peek, and conservation rules to blackjack|V24,V25,V26
T19|x|add resumable one-live-game blackjack lifecycle|V27
T20|x|share island helpers and surface blitz/blackjack UI errors and actions|V28
T11|~|hold showdown for acknowledgement + countdown|V11,V14,V18
T12|x|apply atomic legality, wager bounds, peek, and conservation rules to blackjack|V19,V20,V21
T13|x|add resumable one-live-game blackjack lifecycle|V22
T14|x|share island helpers and surface blitz/blackjack UI errors and actions|V23
T15|x|persist live blackjack games and restore them after restart|V1,V24
T16|x|expire, resume, and limit Hand Blitz runs server-side|V1,V25
T21|x|add blackjack card-counting and strategy trainer settings|V24,V25,V27,V28,V31,V38
T22|x|move blackjack settings to header and simplify game layout|V28,V31,V39
T23|x|persist blackjack shoes with cut-card count continuity and visualization|V38,V41
T24|x|keep compact mobile seat rows visible and badge-safe|V45
T25|x|unify responsive button sizing and label containment|V45,V46
T26|x|center compact seat names and restore readable blackjack action labels|V42,V45,V46
T27|x|replace fold/all-in dialogs with hold actions|V34,V47
T28|x|pin poker edge actions + compact portrait table rows|V42,V44,V47,V48
T29|x|stabilize coin-menu controls across bank updates|V31,V49
T30|x|remove duplicated iPhone safe-area padding|V48,V50
T31|x|remove no-limit wager cap + compact protected actions|V34,V51
T32|x|separate eliminated tournament payout seat from active viewer|V52
T33|x|stabilize narrow poker state + bot re-raise policy|V53
T34|x|emulate the real iPhone PWA viewport and safe-area insets in e2e|V42,V54
T35|x|full-bleed status bar + reserved centre rail for result and equity|V36,V37,V45,V54,V55
T36|x|close betting when no opponent can answer a raise|V47,V51
T37|x|add a custom wager slider beside All In|V47,V56
T38|x|reserve a mid-hand cash seat instead of taking a live one|V1,V12,V57
T39|x|link player names to their page and let people gift $1,000 chips|V1,V2,V58
T40|x|run the all-in board out as advanced state, not a faked reveal|V33,V59
T41|x|net out gifts per counterparty on the player page|V58
T42|x|give the viewer's own hand a panel: cards left, name/stack/wager beside them|V15,V16,V48
T43|x|hold the table's shape as cards, metrics and results come and go|V48,V53
T44|x|redesign other-player seat component across viewports|V14,V20,V43,V44,V45,V53,V54,V61
T45|x|add live table emotes|V3,V30,V42,V53,V61,V63
T46|x|replace the personal blackjack game with four shared fixed-stake tables, quarter-step wagers, multi-seat play and turn clocks|V24,V27,V63
T47|x|make the phone's insets an app-wide contract and unclip landscape|V42,V45,V46,V50,V54
T48|x|fill the five-handed portrait seat grid and cap the phone log|V22,V37,V48,V53,V64

## §B Bug log

- Limit raises were omitted from legal actions while facing a wager; fixed with fixed limit wager bounds and amount validation.
- Wager amounts were checked only by enum discriminant; fixed with legal min/max/fixed validation.
- Short all-in raises could wedge a street and did not give prior callers a chance to call; fixed with explicit `must_call` state.
- All-in had a separate rules path; fixed by normalizing it into call/bet/raise processing.
- Querying legal actions on a completed/arbitrary hand could panic; fixed by returning `Option<LegalActions>`.
- Uncalled excess wagers were not explicitly refunded; fixed before fold/showdown pot formation.
- Street advancement assigned `current_player` twice and could stop with no actor when all players were all-in; fixed with one assignment and automatic runout.
- Short blind posting and action order were not covered; fixed with stack-capped blind posting and regression tests.
- Dense player indexes did not map sparse table seats; fixed with `Player::seat` and seat-aware ordering/awards.
- Seven-card evaluation allocated all 21 combinations; fixed with fixed-index recursive enumeration.
- `SeatView` was duplicated and spectator views could panic; consolidated views and made viewer optional.
- Cents lived in `table.rs` and bot occupants used strings; moved money into `money.rs` and introduced `BotKind`.
- Concurrent no-debt buy-ins could both pass a stale balance check; fixed by checking and appending under one bank lock.
- Bot account filenames used debug formatting; fixed with stable `BotKind` slugs and `FromStr`.
- Tournament stacks were incorrectly eligible for cash-out on leave/replacement; fixed by forfeiting tournament stacks and reserving bank movement for buy-ins/prizes.
- Tournament table listings were indistinguishable from cash tables; fixed with explicit lobby mode labels.
- Table SSR rendered a duplicate island fallback in browsers; fixed by placing the plain fallback inside `noscript`.
- Raw cents inputs and hard-coded frontend stakes hid the actual game configuration; fixed with dollar inputs and boundary conversion plus configurable schedule fields.
- Mobile table controls could run below the viewport and seats overlapped the felt grid; fixed with positioned seats and a fixed mobile action bar.
- Bank widget navigation exposed raw JSON and interpolated ledger text as HTML; fixed with a toggle button and DOM text nodes.
- Felt seat placement could collide with the board/status center and cash bots could remain busted forever; fixed with a reserved center/ring layout and automatic cash-table bot rebuys while preserving tournament eliminations.
- Leaving during a live hand could cash out stale pre-hand chips, and mid-hand rebuys could be overwritten at settlement; fixed with fold-and-pending-departure settlement and rejecting rebuys during active hands.
- Betting rounds could complete while players still owed a check or call (skipping the big blind's preflop option, ending checked streets after one check, and skipping pending calls behind a bet when only one non-all-in player remained); fixed with a single `needs_action` predicate driving both actor rotation and round completion (see STATECHART.md).
- Rejected wagers logged phantom hand events; fixed by validating legality and wager bounds before logging or moving chips.
- An offered all-in could be rejected once the wager cap was hit (legality was checked on the normalized action), and conversely a limit all-in above a call could add a fifth wager to a capped street; fixed by validating the submitted action and gating the all-in offer on the cap.
- Side pots dropped dead money when the highest contributor had folded; fixed by keying pot levels on live contributions and folding all contributions (plus any excess dead money) into the pots.
- The lone player with chips was prompted to act on every street after all opponents were all in; fixed via the `contested` guard so the board runs out instead.
- Tournament cash sit-downs could bypass registration, tournament payouts could index elimination order backwards, and post-start departures could stop dealing; fixed with dedicated registration, explicit payout positions, exact pool distribution, and separate seats-sold/start gating.
- Tournament bots incorrectly used cash table chip limits; fixed by charging the configured money buy-in while assigning starting tournament chips.
- Driver errors on one table could starve later tables; fixed by sorted per-table sweeps that log and continue.
- B1|2026-08-16|Hold'em deals reused sequential `hand_no` as shuffle seed, making same-numbered hands across tables share decks|V29
- Sub-dollar stake display and hidden create-form fields were incorrect; fixed with cent formatting and explicit hidden-label CSS.
- Deferred departures could cash out tournament chips, and out-of-turn departure folds could bypass engine turn bookkeeping; fixed with mode-aware forfeiture and an engine-level arbitrary-seat fold transition.
- Bot seating submitted an empty kind from browser option elements, hid 400 responses, allowed human-seat replacement, and ignored cash no-debt rules; fixed with explicit option values, visible table errors, occupant guards, and propagated no-debt enforcement.
- Card-face refinements stacked new ace and court treatments over older pseudo-element artwork, while a later rule reset corner ranks to medium weight; fixed by consolidating the shared styles and enforcing V7 in browser snapshots.
- The card-test grid fixed every suit to 13 columns and used horizontal overflow on narrow screens; fixed by wrapping centered cards at their in-game size and enforcing V8 at both snapshot widths.
- Rock sent premium hands through a check/call-only helper, making that personality incapable of aggression; fix with wager-first premium play and a deterministic all-policy aggression corpus.
- The tournament payout fixture's tiny terminal blinds could outlive its tick cap once bots played more hands; fixed by using a decisive terminal test level while preserving payout/conservation assertions.
- The frontend asset contract still required the removed variable buy-in form label and later retained client seat payloads; replaced both with fixed-price display and server-selected empty payload assertions under V12/V23.
- The fixed-buy-in route regression double-counted live blinds by adding the pot to pre-hand table stacks; corrected it to assert each authoritative seated stack directly.
- Seats, empty-seat controls, board, metrics, and wagers shared one absolute-positioned ellipse while viewer cards/actions lived below it; replace with outer player rail, owner-attached cards, button-only actions, and unified table log under V14.
- Table asset contracts still required removed range-era labels/formatter; align contracts to player tooltip + whole-dollar `money()` under V14.
- `.seat span` muted every nested card-face child, fading viewer pips/art; scope card colors at seat-card boundary and enforce adjustable saturated viewer cards under V15.
- Asset contract forbade all range inputs to prevent wager slider, blocking card-size control; narrow guard to wager slider/input identifiers under V15.
- Viewer wager used a right-edge exception while every other wager sat above its player; remove the exception and enforce wager-over-cards geometry under V16.
- Three-second automatic redeal hid showdown before users could read it; extend the server-owned pause and expose an acknowledged countdown under V18.
- `.seat b` overrode nested card ranks with accent gold and enlarged cards could cover the viewer wager; enforce card-face color and wager layering under V19.
- Starting a new hand directly in showdown acknowledgement bypassed normal driver follow-up ordering; acknowledgement now expires the shared deadline and lets the driver deal.
- Undealt board slots rendered as dark input-like boxes; render only dealt community cards under V19.
- Player tooltips always opened above their seat, pushing top-rail details beyond the viewport; choose an inward placement and enforce viewport containment under V20.
- Viewer card/rank controls scaled outer cards and corners without reserving table or face space, allowing max settings to cover the board and pips; expand the player rail and reflow card centers under V21.
- The table log used only `max-height`, so its footprint grew with each event and pushed lower controls down; reserve a fixed responsive height under V22.
- Human join forms exposed seat selection, table controls appeared for spectators, and live-hand leave returned success while leaving the same controls visible; move seat assignment server-side and expose one viewer-state command with explicit pending departure under V23.
- Blackjack debited double, split, and insurance before validation and refunded rejected
  actions into the ledger; validate and mutate under the game lock, then charge once.
- Blackjack action flags, handler wager calculations, and store predicates diverged;
  use one server-owned legality and wager source.
- Blackjack never peeked for dealer naturals, incorrectly paying player naturals
  against a dealer natural; peek at deal/insurance boundaries and treat both naturals as push.
- Blackjack starts abandoned live bets on reload and retained finished games forever;
  reject concurrent starts, resume live games, and prune finished user games.
- Blackjack rendered unavailable disabled controls and Hand Blitz hid server errors;
  conditionally render legal actions and share response/error helpers across islands.
- Blackjack exposed the dealer hole card after a non-natural insurance peek; keep
  in-progress responses redacted until resolution.
- Blackjack charged the start bet before atomically rejecting an invalid or live
  start; create the game first and charge only after successful validation.
- Blackjack live games existed only in memory; persist live state atomically and
  restore it on startup while dropping finished games.
- Blackjack rebuilt a fresh shoe for every hand and reset the card-counting
  tutor; persist one shoe per user, carry the settled count forward, and
  reshuffle at the configured cut card.
- Hand Blitz runs never expired without an answer and could pin a charged buy-in
  indefinitely; sweep overdue rounds server-side and prune finished runs.
- Hand Blitz charged its buy-in before rejecting a concurrent live start; create
  the run first and charge only after successful validation.
- Blackjack payouts and Hand Blitz wins accepted non-positive amounts; reject
  zero and negative awards without imposing an upper bound.
- Live table state omitted the viewer's bank balance, so affordable cash buy-in
  buttons rendered disabled outside mocks; include the authenticated balance in
  `TableView` and cover the real join flow under V5/V23.
- Driver ticks updated every table every 250ms even when no hand action, deal,
  or settlement was due; preflight mutation cases before `TableStore::update`
  and cover idle ticks under V30.
- Blackjack, poker table commands, Hand Blitz, and the bank widget kept separate
  balance state, so same-page re-ups and game buy-ins/payouts left sibling UI
  stale; share `bank:updated` account events and cover the flow under V31.
- Fish could randomly fold a made hand from a free `Fold`/`Check` legal-action
  set; normalize app bot policy output so every bot checks instead under V32.
- Terminal tournaments projected `finished` and champion state as soon as the
  final hand settled, letting the UI spoil the winner before reveal; embargo the
  public tournament result until the final hand pause completes under V33.
- All-in odds flex-wrapped into multiple center rows and pushed board/result
  content toward the viewer cards; keep odds to one compact horizontal row under V37.
- Blackjack analyzer treated declining insurance as Stand for the active hand,
  so a hard 5 hit could be called wrong; analyze insurance separately under V40.
- Mobile game CSS let poker inherit desktop viewer-card height expansion and
  let generic action-grid rules override blackjack controls, causing internal
  scroll, tiny cards, inconsistent action sizing, and label overflow; isolate
  mobile game geometry and action contracts under V42.
- Completed desktop showdown badges hung outside player boxes without being
  included in the rail clearance contract, so winners could overlap table-center
  content; include badges in the desktop geometry invariant under V43.
- All-in confirmation only wrapped the literal All In button, so a Call that
  consumed the actor's remaining stack bypassed the warning; classify all-in
  calls by stack commitment under V34.
- Action bars hard-coded poker's maximum action count while blackjack used
  auto-fit columns, leaving short bars partially empty; opponent seat cards
  could also visually cover wager badges. Drive shared action grids from
  visible action count and cover row/layer visibility under V44.
- Compact mobile seats fixed their height and clipped overflow, slicing wagers
  and outcome badges while corner dealer/blind badges could cover long names;
  size rows to their content, place compact badges in flow, and reserve their
  clearance under V45.
- Button surfaces independently overrode height, padding, and wrapping at
  different breakpoints, so narrow action bars and confirmation footers could
  clip or raggedly wrap labels; centralize readable standard and dense action
  control tiers and use a predictable stacked mobile footer under V46.
- Compact in-flow dealer/blind badges still inherited desktop name clearance,
  left-aligning and truncating mobile opponent names; scope clearance to
  absolutely positioned badges while preserving it for compact viewer seats
  under V45.
- Blackjack action buttons consumed poker's sub-`.5rem` dense font tier despite
  having fewer columns, making Hit/Stand and related labels unreadable; give
  blackjack its own readable shared-contract tier under V42/V46.
B2|2026-08-23|bank update replaced clicked menu DOM → Safari retained stale paint|V49
B3|2026-08-23|confirmation dialogs duplicated action state + destabilized bar slots|V34,V47
B4|2026-08-23|mobile stage absorbed spare height while fixed viewer row clipped cards|V48
B5|2026-08-23|mobile action kept bottom safe inset + page repeated inset below footer|V50
B6|2026-08-23|fixed-limit four-wager cap leaked into no-limit + cap note changed action geometry|V51
B7|2026-08-24|eliminated tournament occupant restored as active viewer after deploy|V52
B8|2026-08-24|mobile result reflow moved action bar; bots could min-raise loop|V53
B9|2026-08-24|mobile snapshots emulated a Pixel 7 with zero insets, hiding stage overflow on iPhone PWA heights 761-843 and blackjack hand overlap|V42,V54
B10|2026-08-24|12ce6a8 put compact dealer/blind badges back in flow, spending a row per seat, and the empty showdown result reserved a full-width band above the viewer seat|V45,V55
B11|2026-08-24|a full-raise shove left must_call clear, so a caller facing an all-in shorter stack was still offered Raise and All In with nobody able to answer|V47,V51
B12|2026-08-25|standalone iOS exposed transparent root canvas + zero bottom inset, blackening status area and clipping lifecycle controls|V54
B13|2026-08-27|joining a cash table mid-hand swapped the newcomer into a house player's live seat, so they played a hand they never paid into and settlement wrote that seat's hand-final stack over their buy-in, destroying the difference and paying the displaced bot its stale pre-hand stack|V1,V57
B14|2026-08-28|blackjack sized every card at a fixed 29cqw share of its hand, so a fourth card pushed the row past the hand's width and a long mobile hand ran off screen; the play area also gave its two explicit rows all the height, collapsing split hands three and beyond to nothing|V42
B15|2026-08-28|all-in runout resolved synchronously at hand end (`enter_betting_round` → `advance_street` recursion), so the whole result existed before the reveal & each spoiler-carrying field needed its own embargo; `viewer_seat`/`viewer_eliminated` had none, so a busted tournament player lost their seat mid-reveal & jumped to the opponent row before the board was out|V33,V59
B16|2026-08-31|the one-card zoom rule matched the viewer's own hole cards as well as the board, so hovering one card grew it 1.35x on top of the hand's own scale and left its partner behind, against V17's "either viewer card magnifies both"|V15,V17
B17|2026-08-31|card zoom hung on :hover, which iOS leaves stuck on the last thing tapped, so a magnified hand outlived the tap and covered the action row the next press was aimed at|V15
B18|2026-08-31|a seat rendered no card element between hands, so every seat lost a grid row when a hand ended and the mobile decision area jumped 37px up into the finger already travelling toward the action row|V53
B19|2026-09-02|the viewer's panel was centred and hugged its contents, so a hand ending narrowed its card column and a longer stack widened the figures beside it, sliding the hand ~30px sideways; and the metrics left the felt between hands, shortening it and lifting the same panel up the screen|V48,V53
B20|2026-09-02|opponent redesign changed card + tile height on reveal, moving table geometry|V53,V61
B21|2026-09-03|a person's cash-table buy-in and rebuy both passed `no_debt: true` regardless of the table's own mode, so V10's buy-in auto-loan never fired for a human: a balance under the buy-in was refused outright and the lobby filed the $500/$1,000 rungs under "out of reach" instead of lending the shortfall|V5,V10
B22|2026-09-03|the fix for B21 lent at every rung, so a broke player could borrow their way into the deepest table on the ladder and owe a hundred loans for one buy-in; lending now stops at the $1,000 seat for people, while the house stays staked everywhere|V10,V16
B23|2026-09-04|the design-system split reintroduced `100dvh` the day after V54 pinned `100vh`, and pinned the tablet stage height on width alone: a landscape phone is >641px wide, so it kept a 522pt stage inside a 321pt shell and `overflow:hidden` swallowed 312px — the viewer's hand, the whole action bar and the footer were all below the fold, and the broken frame shipped as the landscape baseline in the same commit. L4 stayed green throughout because it asked the stage whether it scrolled (it did not; it overflowed its parent) and the document whether it scrolled (it did not; the shell clipped)|V42,V54
B24|2026-09-04|safe-area insets were only ever wired into the poker table, one bug at a time; every other surface kept the plain 1rem gutter, so a landscape phone put blackjack's bet row, the lobby's rungs and the player page's controls behind the Dynamic Island, and Hand Blitz's fixed shell set `padding-bottom:0` and reserved nothing above the home indicator|V54
B25|2026-09-04|blackjack's landscape play area stacked dealer, seat strip and your own hands with the strip on an `auto` track, so the strip took the whole area and both hands collapsed to nothing — B14's failure from the other side. It went unseen because the blackjack rewrite made every table test desktop-only and left the phone one 412x915 check with every inset at zero, asserting only that nothing scrolled sideways|V42,V54
B26|2026-09-04|the emote taps shipped at a fixed 2rem square on the same footer row as 44px History and Leave, below the tap target every other control on that row keeps|V46,V63
B27|2026-09-04|a revealed opponent hand is taller than a face-down one and the cards carry a `z-index`, so at a showdown an all-in seat's own cards grew down out of their track and over the ALL IN chip in the strip below them; the hit test that would have caught it was only ever run against a live flop, where the cards are small|V45
