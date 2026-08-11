import { wholeDollarMoney } from "/public/shared.js";

const balance = document.getElementById("bank-balance");
const delta = document.getElementById("bank-delta");
const widget = document.querySelector(".bank-widget");

if (balance && widget) {
  fetch("/api/bank", { headers: { Accept: "application/json" } })
    .then((response) => (response.ok ? response.json() : null))
    .then((account) => {
      if (!account) return;
      balance.textContent = wholeDollarMoney(account.balance);
      const latest = account.entries.at(-1);
      if (latest) delta.textContent = ` (${latest.delta >= 0 ? "+" : ""}${wholeDollarMoney(latest.delta)})`;
      const panel = document.createElement("span");
      panel.className = "bank-panel";
      panel.setAttribute("role", "status");
      const heading = document.createElement("strong");
      heading.textContent = `Balance ${wholeDollarMoney(account.balance)}`;
      panel.append(heading);
      for (const entry of account.entries.slice(-5).toReversed()) {
        const line = document.createElement("div");
        line.textContent = `${entry.delta >= 0 ? "+" : ""}${wholeDollarMoney(entry.delta)} ${entry.memo}`;
        panel.append(line);
      }
      widget.append(panel);
      widget.addEventListener("click", () => {
        const expanded = widget.getAttribute("aria-expanded") === "true";
        widget.setAttribute("aria-expanded", String(!expanded));
      });
    })
    .catch(() => {});
}
