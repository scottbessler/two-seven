function cents(value) {
  return Math.round(Number(value) * 100);
}

const form = document.getElementById("create-table-form");
if (form) {
  const sync = () => {
    const limit = form.elements.limit.value === "limit";
    for (const name of ["small_blind", "big_blind"]) form.elements[name].closest("label").hidden = limit;
    for (const name of ["small_bet", "big_bet"]) form.elements[name].closest("label").hidden = !limit;
  };
  form.elements.limit.addEventListener("change", sync);
  sync();
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const data = new FormData(form);
    const limit = data.get("limit") === "limit";
    const stakes = limit
      ? { Limit: { small_bet: cents(data.get("small_bet")), big_bet: cents(data.get("big_bet")) } }
      : { NoLimit: { small_blind: cents(data.get("small_blind")), big_blind: cents(data.get("big_blind")) } };
    const response = await fetch("/tables", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: data.get("name"), stakes, no_debt: data.get("no_debt") === "on", min_buy_in: cents(data.get("min_buy_in")), max_buy_in: cents(data.get("max_buy_in")) }),
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
    const levels = [1, 2].map((number) => ({
      small_blind: cents(data.get(`small_blind_${number}`)),
      big_blind: cents(data.get(`big_blind_${number}`)),
      ante: cents(data.get(`ante_${number}`)),
      hands: Number(data.get(`hands_${number}`)),
    }));
    const response = await fetch("/tournaments", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: data.get("name"), buy_in: cents(data.get("buy_in")), seat_count: Number(data.get("seat_count")), starting_chips: Number(data.get("starting_chips")), no_debt: data.get("no_debt") === "on", levels, payout_percentages: data.get("payouts").split(",").map((value) => Number(value.trim())) }),
    });
    if (response.ok) window.location.href = (await response.json()).url;
    else document.getElementById("create-error").textContent = "Unable to create tournament";
  });
}
