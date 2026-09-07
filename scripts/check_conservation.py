#!/usr/bin/env python3
"""Check SPEC §V1, §V2, and §V4 invariants in a JSON DATA_PATH."""
import glob
import json
import os
import sys

root = sys.argv[1] if len(sys.argv) > 1 else "/tmp/two-seven-e2e"
failures = []
accounts = []
bank_total = 0

for path in sorted(glob.glob(os.path.join(root, "bank", "*.json"))):
    account = json.load(open(path))
    running = 0
    for entry in account["entries"]:
        running += entry["delta"]
        if entry["balance_after"] != running:
            failures.append(f"V2 {path}: balance_after does not match running ledger")
    if account["balance"] != running:
        failures.append(f"V2 {path}: balance does not match ledger sum")
    accounts.append(account)
    bank_total += account["balance"]

collected = {}
prizes = {}
cashouts = {}
for account in accounts:
    for entry in account["entries"]:
        kind = entry["kind"]
        if "BuyIn" in kind:
            table = kind["BuyIn"]["table"]
            collected[table] = collected.get(table, 0) - entry["delta"]
        elif "TournamentPrize" in kind:
            tournament = kind["TournamentPrize"]["tournament"]
            prizes[tournament] = prizes.get(tournament, 0) + entry["delta"]
        elif "CashOut" in kind:
            table = kind["CashOut"]["table"]
            cashouts[table] = cashouts.get(table, 0) + entry["delta"]

cash_stacks = 0
pots = 0
escrow = 0
for path in sorted(glob.glob(os.path.join(root, "tables", "*.json"))):
    table = json.load(open(path))
    tournament = table.get("mode", {}).get("Tournament")
    seats = sum(seat["stack"] for seat in table["seats"])
    pot = 0
    hand = table.get("hand")
    if hand:
        pot = hand["pot"]
        live_seats = {player["seat"] for player in hand["players"]}
        seats -= sum(table["seats"][seat]["stack"] for seat in live_seats)
        seats += sum(player["stack"] for player in hand["players"])
    if tournament is None:
        cash_stacks += seats
        pots += pot
    else:
        tournament_id = table["id"]
        escrow += collected.get(tournament_id, 0) - prizes.get(tournament_id, 0)
        if cashouts.get(tournament_id, 0):
            failures.append(f"V1 {table['name']}: tournament has cash-out entries")
        expected = tournament["registered"] * tournament["config"]["buy_in"]
        if collected.get(tournament_id, 0) != expected:
            failures.append(f"tournament {table['name']}: buy-ins do not match registrations")
        if tournament["prize_pool"] != expected:
            failures.append(f"tournament {table['name']}: prize pool does not match buy-ins")

    for label, summary in (
        ("last_hand", table.get("last_hand")),
        ("live_summary", (hand or {}).get("summary")),
    ):
        if summary:
            awarded = sum(award["amount"] for award in summary["awards"])
            contributed = sum(summary.get("contributions", {}).values())
            if awarded != contributed:
                failures.append(
                    f"V4 {table['name']} {label}: awards {awarded} != contributions {contributed}"
                )

# Chips bought at a house game have left the bank and are sitting at a table,
# so §V1 only balances if they are counted back. Roulette keeps a player's whole
# worth in `stack` -- chips on the felt are a claim on it, not a withdrawal --
# and a blackjack game spreads one across its stack, its live bets and insurance.
house_stacks = 0
roulette_path = os.path.join(root, "roulette", "tables.json")
if os.path.exists(roulette_path):
    for table in json.load(open(roulette_path)):
        house_stacks += table["stack"]

blackjack_path = os.path.join(root, "blackjack", "solo.json")
if os.path.exists(blackjack_path):
    for game in json.load(open(blackjack_path)):
        house_stacks += game["stack"] + game.get("insurance", 0)
        house_stacks += sum(hand["bet"] for hand in game.get("hands", []))

total = bank_total + cash_stacks + pots + escrow + house_stacks
if total:
    failures.append(f"V1 total is {total}, expected zero")

if failures:
    print("\n".join(f"FAIL: {failure}" for failure in failures))
    raise SystemExit(1)
print("All conservation checks passed (V1, V2, V4).")
