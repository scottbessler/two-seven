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
