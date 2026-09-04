# Storage

Game state lives in SQLite, on the Fly volume at `DATA_PATH`, replicated to
Tigris. This document says why, and what is still on the JSON tree.

## Where it started

Seven stores each loaded a directory of JSON at boot and rewrote a whole file
on every change: tables, users, bank accounts, blackjack games and shoes,
blitz stats, player stats, and hand history. It worked, and for a single
machine it was fast, but three things were wrong with it.

*Nothing was fsynced.* Every store wrote a temp file and renamed it. That is
atomic against a process crash and says nothing about a machine crash — the
rename can land before the bytes do.

*Reads were unbounded.* `history/<table>.jsonl` had to be parsed end to end to
answer either question the history page asks, so a table's thousandth hand
cost a thousand hands of work. `stats/players.json` and `blitz/stats.json`
rewrote the entire global map on every update.

*The volume was the only copy.* One machine, one volume, no replica. A lost
volume was a lost database.

## Why not object storage directly

Tigris is the obvious answer to the third problem, and the wrong answer to it
alone. `TableStore::persist` fires on every fold, call, and raise, mid
websocket; a six-handed hand is twenty to forty full-document writes. Putting
a network round trip — a billed one — in that path trades microseconds for
milliseconds on the most latency-sensitive code in the app.

Object storage is the right destination for this data, not the right
interface to it. So: SQLite on local disk for the hot path, and Litestream
shipping the write-ahead log to Tigris behind the process. Recovery is a
`litestream restore` into a fresh machine before the binary starts.

LiteFS is the heavier alternative, and buys read replicas across machines. We
have one machine. Not yet.

## Why not RocksDB

It would drag a C++ toolchain into a `debian:bookworm-slim` image for a
dependency tree that is otherwise pure Rust, and give up ad-hoc queries to do
it. The queries are most of the point: hand history pagination, the stats
aggregates, and `scripts/check_conservation.py` all want SQL. If the goal
were only a key-value store, `redb` or `fjall` would be the pure-Rust
answers — but it isn't.

## The shape

`src/db.rs` owns the connection. One connection, on the blocking pool, behind
a mutex: the same single writer the JSON stores assumed. WAL, `synchronous =
NORMAL`, `busy_timeout`. Readers queue behind writers, which costs nothing at
this size — the reads being replaced walked whole files — and a read pool is
the answer when it stops being true, not before.

Migrations are an append-only list in `src/db.rs`; `PRAGMA user_version`
counts how many have been applied. A migration that has shipped is never
edited.

**Documents stay documents.** A `HandRecord` is one JSON blob in one row, not
a normalized schema. It is always read whole, and leaving it serialized means
the Rust types keep changing shape without a migration. Normalization is
worth it only where a query wants the columns.

## Moving the stores over

Each store is carried over on its own, and each import is recorded in
`legacy_imports` in the same transaction as the rows it moved — so a crash
halfway leaves the JSON tree authoritative and the next boot starts over. The
JSON files are left in place after an import; they are the fallback until the
database has proven itself on a deployed machine.

- [x] `HistoryStore` — hands, keyed by table and hand number. The worst
      asymptotics and the smallest surface, so it went first.
- [ ] `BankStore` — the ledger is already append-only and tabular; this is
      the one that most wants real columns, and it makes
      `scripts/check_conservation.py` three queries instead of a directory
      walk.
- [ ] `StatsStore`, `BlitzStore` — stop rewriting a global map per update.
- [ ] `BlackjackStore`, `UserStore` — document rows.
- [ ] `TableStore` — last, and stays a document blob. The hot path, so it
      moves once everything quieter has proven the shape.

Once more than one store is on the database, `app.rs` opens the `Db` once and
hands the same handle to each; today `HistoryStore::load` opens it, because
it is the only one.

## Replication (not yet wired)

The remaining piece is a Litestream sidecar in the image and a restore in
`start.sh`:

```yaml
# litestream.yml
dbs:
  - path: /data/two-seven.db
    replicas:
      - type: s3
        endpoint: https://fly.storage.tigris.dev
        bucket: $BUCKET_NAME
```

`fly storage create` provisions the bucket and sets `AWS_ACCESS_KEY_ID`,
`AWS_SECRET_ACCESS_KEY`, and `BUCKET_NAME` as secrets. `start.sh` becomes
`litestream restore -if-db-not-exists -if-replica-exists /data/two-seven.db`
then `litestream replicate -exec ./two-seven`, so the replication process
owns the binary's lifetime and the WAL keeps shipping until it exits.
