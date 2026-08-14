// Buy-in drives everything else: no-limit blinds, limit bet sizes, and the
// stack you sit down with. Amounts are cents.
const BUY_INS = {
  20_000: { label: "$200", blinds: [100, 200], limit: [200, 400] },
  50_000: { label: "$500", blinds: [200, 400], limit: [500, 1_000] },
  100_000: { label: "$1,000", blinds: [500, 1_000], limit: [1_000, 2_000] },
  200_000: { label: "$2,000", blinds: [1_000, 2_000], limit: [2_000, 4_000] },
};

const CASH_SEATS = 6;

// Tournaments always deal 10,000 chips and climb the T10,000 ladder from
// homepokertourney.org. Chips are cents, so a 100-chip blind is 10,000.
const TOURNAMENT_CHIPS = 1_000_000;
const TOURNAMENT_BLINDS = [
  [100, 200],
  [200, 400],
  [300, 600],
  [400, 800],
  [500, 1_000],
  [600, 1_200],
  [800, 1_600],
  [1_000, 2_000],
  [1_500, 3_000],
  [2_000, 4_000],
  [3_000, 6_000],
  [4_000, 8_000],
  [5_000, 10_000],
  [6_000, 12_000],
  [8_000, 16_000],
];

const PAYOUTS = { 4: [100], 6: [65, 35], 9: [50, 30, 20] };

function tournamentLevels(players) {
  // A level lasts twice as many hands as there are players, so every seat
  // takes the blinds twice before the stakes climb.
  const hands = players * 2;
  return TOURNAMENT_BLINDS.map(([smallBlind, bigBlind]) => ({
    small_blind: smallBlind * 100,
    big_blind: bigBlind * 100,
    ante: 0,
    hands,
  }));
}

export function gameRequest({ format, betting, players, buyIn }, name, noDebt) {
  // Choices arrive as strings from the option buttons; the API wants numbers.
  const amount = Number(buyIn);
  const tier = BUY_INS[amount];
  if (!tier) throw new Error("Pick a buy-in");
  if (format === "tournament") {
    const seats = Number(players);
    return {
      endpoint: "/tournaments",
      body: {
        name,
        no_debt: noDebt,
        buy_in: amount,
        seat_count: seats,
        starting_chips: TOURNAMENT_CHIPS,
        levels: tournamentLevels(seats),
        payout_percentages: PAYOUTS[seats],
      },
    };
  }
  const stakes = betting === "limit"
    ? { Limit: { small_bet: tier.limit[0], big_bet: tier.limit[1] } }
    : { NoLimit: { small_blind: tier.blinds[0], big_blind: tier.blinds[1] } };
  return {
    endpoint: "/tables",
    body: { name, no_debt: noDebt, stakes, max_seats: CASH_SEATS, buy_in: amount },
  };
}

export function summarize({ format, betting, players, buyIn }) {
  const tier = BUY_INS[Number(buyIn)];
  if (format === "tournament") {
    const paid = PAYOUTS[Number(players)].length;
    return `${tier.label} tournament · ${players} players · 10,000 chips · top ${paid} paid`;
  }
  const stakes = betting === "limit"
    ? `$${tier.limit[0] / 100}/$${tier.limit[1] / 100} limit`
    : `$${tier.blinds[0] / 100}/$${tier.blinds[1] / 100} no-limit`;
  return `${tier.label} cash game · ${stakes} · ${CASH_SEATS} seats`;
}

function stepsFor(choices) {
  return ["format", choices.format === "tournament" ? "players" : "betting", "buyIn", "confirm"];
}

const form = document.getElementById("quick-game-form");
const dialog = document.getElementById("game-setup");
if (form && dialog) {
  const choices = {};
  let index = 0;
  const steps = () => stepsFor(choices);
  const back = form.querySelector(".setup-back");
  const errorEl = document.getElementById("create-error");
  const summary = document.getElementById("setup-summary");

  const show = () => {
    const current = steps()[index];
    for (const step of form.querySelectorAll(".setup-step")) step.hidden = step.dataset.step !== current;
    for (const option of form.querySelectorAll(".setup-option")) {
      option.setAttribute("aria-pressed", String(choices[option.dataset.choice] === option.value));
    }
    back.hidden = index === 0;
    if (current === "confirm") summary.textContent = summarize(choices);
  };

  form.addEventListener("click", (event) => {
    const option = event.target.closest(".setup-option");
    if (!option) return;
    choices[option.dataset.choice] = option.value;
    index = Math.min(index + 1, steps().length - 1);
    show();
  });

  back.addEventListener("click", () => {
    index = Math.max(0, index - 1);
    show();
  });

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const submit = form.querySelector(".setup-create");
    submit.disabled = true;
    errorEl.textContent = "";
    try {
      const data = new FormData(form);
      const request = gameRequest(choices, data.get("name"), data.get("no_debt") === "on");
      const response = await fetch(request.endpoint, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(request.body),
      });
      if (!response.ok) {
        const failure = await response.json().catch(() => null);
        throw new Error(failure?.error || `Unable to create game (HTTP ${response.status})`);
      }
      const result = await response.json();
      if (!result.url) throw new Error("Create game response did not include a destination");
      window.location.href = result.url;
    } catch (error) {
      errorEl.textContent = error instanceof Error ? error.message : "Unable to create game";
      submit.disabled = false;
    }
  });

  show();
  if (!dialog.open) dialog.showModal();
}
