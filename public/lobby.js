const form = document.getElementById("create-table-form");
if (form) {
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const data = new FormData(form);
    const limit = data.get("limit");
    const stakes = limit === "limit"
      ? { Limit: { small_bet: 200, big_bet: 400 } }
      : { NoLimit: { small_blind: 100, big_blind: 200 } };
    const response = await fetch("/tables", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        name: data.get("name"),
        stakes,
        no_debt: data.get("no_debt") === "on",
        min_buy_in: Number(data.get("min_buy_in")),
        max_buy_in: Number(data.get("max_buy_in")),
      }),
    });
    if (response.ok) window.location.href = (await response.json()).url;
    else document.getElementById("create-error").textContent = "Unable to create table";
  });
}
const tournamentForm = document.getElementById("create-tournament-form");
if (tournamentForm) {
  tournamentForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const data = new FormData(tournamentForm);
    const response = await fetch("/tournaments", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        name: data.get("name"),
        buy_in: Number(data.get("buy_in")),
        seat_count: Number(data.get("seat_count")),
        starting_chips: Number(data.get("starting_chips")),
        no_debt: data.get("no_debt") === "on",
        levels: [
          { small_blind: 10, big_blind: 20, ante: 0, hands: 10 },
          { small_blind: 20, big_blind: 40, ante: 5, hands: 10 },
          { small_blind: 40, big_blind: 80, ante: 10, hands: 10 },
        ],
        payout_percentages: [65, 35],
      }),
    });
    if (response.ok) window.location.href = (await response.json()).url;
    else document.getElementById("create-error").textContent = "Unable to create tournament";
  });
}
