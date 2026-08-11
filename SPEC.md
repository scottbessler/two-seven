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
- Cash tables in three flavours: **limit**, **no-limit**, and **"no-debt"**
  variants of either (see §4 Bank).
- Tournaments (single table first).
- A persistent **bank**: every player (and every bot type) has an account. Buying
  in may push a normal account negative (debt); no-debt tables refuse that.
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
Account { owner: AccountOwner, balance: Cents, entries: Vec<LedgerEntry>, created_at, updated_at }
AccountOwner = User(Uuid) | Bot(BotKind)     // one shared account per bot kind
LedgerEntry  { id, at, kind, delta: Cents, balance_after: Cents, memo }
LedgerKind   = BuyIn{table} | CashOut{table} | TournamentBuyIn{tournament}
             | TournamentPrize{tournament} | Adjustment
```

Rules

- A new user's account starts at **$0**. Bot accounts are created lazily, also at $0.
- A normal buy-in may take the balance negative — that is the "going into debt"
  path and it is the default; each gameplay buy-in, entry, or wager ≤ $10,000.
- **No-debt tables** (`no_debt: true`, available for both limit and no-limit):
  a player may only sit down if their balance is `> 0`, and the buy-in must be
  `<= balance`. Rebuys at the table obey the same rule.
- Cash-out returns the seat's remaining stack to the account.
- The bank is the settlement layer: chips only enter play through a `BuyIn` and
  only leave through a `CashOut`/prize, so `sum(balances) + sum(chips in play)`
  is invariant (§V1).
- Every account's `balance` must equal the sum of its ledger deltas (§V2).

UI: the header shows the signed-in user's balance next to their username, with a
coin icon; hovering/tapping it opens a small panel with the current balance and
the most recent ledger deltas. Seat labels at a table show the seat owner's bank
balance the same way (bots included).

## 5. Hold'em rules implemented

- 2–9 seats. Button rotates clockwise each hand; heads-up uses the standard
  button-posts-small-blind rule.
- Streets: preflop, flop (3), turn (1), river (1); one burn card is *not*
  modelled (irrelevant with a shuffled deck).
- Actions: `fold`, `check`, `call`, `bet`, `raise`, plus implicit all-in when a
  player cannot cover.
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
        last_hand: Option<HandSummary>, hand_no, next_action_at, created_at, updated_at }
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

## 7. Bots

One shared bank account per bot kind. Difficulty ladder:

| Kind | Style |
| --- | --- |
| `Fish` | Near-random: calls far too much, raises at random, never folds a pair. |
| `Rock` | Tight-passive: fixed preflop opening ranges, calls with made hands, folds otherwise. |
| `Grinder` | Tight-aggressive: hand strength buckets + pot odds, bets/raises with strong made hands and draws, folds marginal spots. |
| `Shark` | Monte-Carlo equity estimate against the live opponent count, compared with pot odds; bet sizing scales with equity, occasional bluff/semi-bluff. |

Bots see only what a player in that seat legitimately sees (their own hole cards
and the board) — the same redacted view a human gets (§V3).

## 8. Real-time and the driver

- `TableStore` broadcasts the id of any table that changed; `GET /tables/{id}/events`
  streams a redacted `TableView` snapshot immediately and on every change.
- A single background task ticks a few times per second and, for each table,
  performs whatever the clock says is due: act for a bot whose turn it is, or
  deal the next hand. All mutation goes through the same engine entry points the
  HTTP handlers use, so there is one rules path.

## 9. Routes

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/` | Lobby: bank widget, open tables, tournaments, your seats |
| GET | `/healthcheck` | Liveness for Fly |
| POST | `/auth/register/begin`, `/auth/register/finish` | Passkey registration |
| POST | `/auth/login/begin`, `/auth/login/finish` | Passkey login |
| POST | `/auth/logout` | Clear session |
| GET | `/tables/new` | Create-table form |
| POST | `/tables` | Create a table |
| GET | `/tables/{id}` | SSR table page + island mount |
| GET | `/tables/{id}/state` | Redacted `TableView` JSON (SSE fallback) |
| GET | `/tables/{id}/events` | SSE stream of `TableView` |
| POST | `/tables/{id}/join` | Sit down: `{seat, buy_in}` (bank-checked) |
| POST | `/tables/{id}/leave` | Stand up and cash out |
| POST | `/tables/{id}/sit` | `{sitting_out: bool}` |
| POST | `/tables/{id}/rebuy` | Top up the seat stack from the bank |
| POST | `/tables/{id}/bot` | Seat/remove a bot: `{seat, kind?}` |
| POST | `/tables/{id}/action` | `{kind: fold\|check\|call\|bet\|raise, amount?}` |
| GET | `/tournaments/new`, POST `/tournaments` | Create a sit-and-go |
| POST | `/tournaments/{id}/register` | Buy in |
| GET | `/api/bank` | Balance + recent ledger entries |

HTML routes return an escaped error page (`AppError`); JSON routes return
`{"error": "..."}` with 400/401/404/409/422.

## 10. Tournaments (single table, sit-and-go)

- Config: buy-in, seat count, starting chips, blind schedule (level = list of
  `{small_blind, big_blind, ante, hands}`), payout percentages.
- Registering charges the buy-in from the bank (respecting `no_debt` if set) and
  seats the player. Bots can fill the remaining seats.
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
public/                          # app.css, table.js island, auth.js, vendor/
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
  nor any undealt card; bots consume the same projection.
- **V4** Pot distribution pays out exactly the pot: the sum of awards equals the
  sum of contributions, for any all-in/side-pot configuration.
- **V5** No-debt tables never allow an account below zero.
- **V6** Every reachable hand state has at least one legal action for the player
  on turn, and the engine rejects any action not in that set.
- **V7** The shared card face renders all 52 cards with bold corner ranks,
  undecorated centered aces, and unhatched chess-piece portraits for J/Q/K in
  both SSR and island views; desktop and mobile snapshots cover the full deck.
- **V8** Every `/card-test` suit row keeps the in-game card dimensions while
  wrapping all 13 cards within its visible width; no suit row scrolls horizontally.
- **V9** ∀ positive configured stake, blind, ante, buy-in, entry fee, or wager ≥ 100 cents.
- **V10** ∀ single gameplay buy-in, entry, rebuy, or wager ≤ 1,000,000 cents;
  cumulative account debt remains unbounded.
- **V11** `TableView` exposes redacted action events + per-seat hand state; UI shows
  current actor, dealer, blinds, street wager, folded/all-in state, and recent log.
- **V12** Every cash table has one fixed buy-in; human joins, bot seats, and rebuys
  charge and assign exactly that amount, independent of client payloads.
- **V13** Every bot personality produces at least one bet or raise across the
  deterministic aggression corpus while retaining distinct style signals.
- **V14** Desktop + mobile table snapshots keep occupied players outside felt,
  viewer cards at viewer seat, board unobscured, actions before unified log,
  one-line header, whole-dollar labels, and integrated showdown winners/cards.

## §T Build tasks

id|status|task|cites
T1|x|enforce $1 floor + $10k per-entry ceiling|V1,V2,V5,V9,V10
T2|x|replace setup forms with 3 cash + 3 tournament presets|V9
T3|x|add structured hand events + per-seat hand state|V3,V4,V6,V11
T4|x|render game log + table cues|V7,V11
T5|x|replace cash buy-in ranges with one fixed amount|V1,V2,V9,V10,V12
T6|x|restore and regression-test bot aggression|V6,V13
T7|x|recompose live table + showdown UI|V3,V7,V11,V14

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
- Sub-dollar stake display and hidden create-form fields were incorrect; fixed with cent formatting and explicit hidden-label CSS.
- Deferred departures could cash out tournament chips, and out-of-turn departure folds could bypass engine turn bookkeeping; fixed with mode-aware forfeiture and an engine-level arbitrary-seat fold transition.
- Bot seating submitted an empty kind from browser option elements, hid 400 responses, allowed human-seat replacement, and ignored cash no-debt rules; fixed with explicit option values, visible table errors, occupant guards, and propagated no-debt enforcement.
- Card-face refinements stacked new ace and court treatments over older pseudo-element artwork, while a later rule reset corner ranks to medium weight; fixed by consolidating the shared styles and enforcing V7 in browser snapshots.
- The card-test grid fixed every suit to 13 columns and used horizontal overflow on narrow screens; fixed by wrapping centered cards at their in-game size and enforcing V8 at both snapshot widths.
- Rock sent premium hands through a check/call-only helper, making that personality incapable of aggression; fix with wager-first premium play and a deterministic all-policy aggression corpus.
- The tournament payout fixture's tiny terminal blinds could outlive its tick cap once bots played more hands; fixed by using a decisive terminal test level while preserving payout/conservation assertions.
- The frontend asset contract still required the removed variable buy-in form label; replaced it with fixed-price display and seat-only payload assertions under V12.
- The fixed-buy-in route regression double-counted live blinds by adding the pot to pre-hand table stacks; corrected it to assert each authoritative seated stack directly.
- Seats, empty-seat controls, board, metrics, and wagers shared one absolute-positioned ellipse while viewer cards/actions lived below it; replace with outer player rail, owner-attached cards, button-only actions, and unified table log under V14.
- Table asset contracts still required removed range-era labels/formatter; align contracts to player tooltip + whole-dollar `money()` under V14.
