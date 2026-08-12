const PRESETS = {
  "cash-friendly": {
    endpoint: "/tables",
    body: {
      stakes: { NoLimit: { small_blind: 100, big_blind: 200 } },
      max_seats: 6,
      buy_in: 20_000,
    },
  },
  "cash-standard": {
    endpoint: "/tables",
    body: {
      stakes: { NoLimit: { small_blind: 500, big_blind: 1_000 } },
      max_seats: 6,
      buy_in: 100_000,
    },
  },
  "cash-limit": {
    endpoint: "/tables",
    body: {
      stakes: { Limit: { small_bet: 2_000, big_bet: 4_000 } },
      max_seats: 6,
      buy_in: 200_000,
    },
  },
  "tournament-quick": {
    endpoint: "/tournaments",
    body: {
      buy_in: 1_000,
      seat_count: 4,
      starting_chips: 20_000,
      levels: [
        { small_blind: 100, big_blind: 200, ante: 0, hands: 8 },
        { small_blind: 200, big_blind: 400, ante: 100, hands: 8 },
      ],
      payout_percentages: [100],
    },
  },
  "tournament-classic": {
    endpoint: "/tournaments",
    body: {
      buy_in: 5_000,
      seat_count: 6,
      starting_chips: 40_000,
      levels: [
        { small_blind: 100, big_blind: 200, ante: 0, hands: 10 },
        { small_blind: 200, big_blind: 400, ante: 100, hands: 10 },
        { small_blind: 400, big_blind: 800, ante: 200, hands: 10 },
      ],
      payout_percentages: [65, 35],
    },
  },
  "tournament-deep": {
    endpoint: "/tournaments",
    body: {
      buy_in: 20_000,
      seat_count: 9,
      starting_chips: 100_000,
      levels: [
        { small_blind: 100, big_blind: 200, ante: 0, hands: 12 },
        { small_blind: 200, big_blind: 400, ante: 100, hands: 12 },
        { small_blind: 400, big_blind: 800, ante: 200, hands: 12 },
      ],
      payout_percentages: [50, 30, 20],
    },
  },
};

const form = document.getElementById("quick-game-form");
if (form) {
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const submit = form.querySelector("button[type=submit]");
    const errorEl = document.getElementById("create-error");
    if (submit) submit.disabled = true;
    if (errorEl) errorEl.textContent = "";
    try {
      const data = new FormData(form);
      const preset = PRESETS[data.get("preset")];
      if (!preset) throw new Error("Unknown game preset");
      const response = await fetch(preset.endpoint, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          ...preset.body,
          name: data.get("name"),
          no_debt: data.get("no_debt") === "on",
        }),
      });
      if (!response.ok) {
        const error = await response.json().catch(() => null);
        throw new Error(error?.error || `Unable to create game (HTTP ${response.status})`);
      }
      const result = await response.json();
      if (!result.url) throw new Error("Create game response did not include a destination");
      window.location.href = result.url;
    } catch (error) {
      if (errorEl) errorEl.textContent = error instanceof Error ? error.message : "Unable to create game";
      if (submit) submit.disabled = false;
    }
  });
}
