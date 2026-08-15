import { wholeDollarMoney as money } from "/public/shared.js";

if ("serviceWorker" in navigator) navigator.serviceWorker.register("/sw.js").catch(() => {});

const balance = document.getElementById("bank-balance");
const delta = document.getElementById("bank-delta");
const widget = document.querySelector(".bank-widget");

if (balance && widget) {
  const loadBank = () => fetch("/api/bank", { headers: { Accept: "application/json" } })
    .then((response) => (response.ok ? response.json() : null))
    .then((account) => {
      if (!account) return;
      balance.textContent = money(account.balance);
      const latest = account.entries.at(-1);
      delta.textContent = latest ? ` (${latest.delta >= 0 ? "+" : ""}${money(latest.delta)})` : "";
      for (const button of document.querySelectorAll(".re-up-form button")) {
        button.disabled = account.balance >= 10_000;
      }
      let panel = widget.querySelector(".bank-panel");
      if (!panel) {
        panel = document.createElement("span");
        panel.className = "bank-panel";
        panel.setAttribute("role", "status");
        widget.append(panel);
      }
      panel.replaceChildren();
      const heading = document.createElement("strong");
      heading.textContent = `Balance ${money(account.balance)}`;
      panel.append(heading);
      const shame = document.createElement("span");
      shame.className = "loan-badge";
      shame.textContent = `Loans ${account.loan_count ?? 0}`;
      panel.append(shame);
      const reUp = document.createElement("button");
      reUp.type = "button";
      reUp.className = "re-up-button";
      reUp.textContent = "Re-up $1,000";
      reUp.disabled = account.balance >= 10_000;
      reUp.addEventListener("click", (event) => {
        event.stopPropagation();
        reUp.disabled = true;
        fetch("/api/bank", {
          method: "POST",
          headers: { Accept: "application/json", "Content-Type": "application/json" },
          body: "{}",
        })
          .then((response) => (response.ok ? response.json() : Promise.reject(response)))
          .then((updated) => {
            account = updated;
            return loadBank();
          })
          .catch(() => {
            reUp.disabled = account.balance >= 10_000;
          });
      });
      panel.append(reUp);
      for (const entry of account.entries.slice(-5).toReversed()) {
        const line = document.createElement("div");
        line.textContent = `${entry.delta >= 0 ? "+" : ""}${money(entry.delta)} ${entry.memo}`;
        panel.append(line);
      }
    })
    .catch(() => {});
  loadBank();
  for (const form of document.querySelectorAll(".re-up-form")) {
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      fetch("/api/bank", {
        method: "POST",
        headers: { Accept: "application/json", "Content-Type": "application/json" },
        body: "{}",
      }).then(() => loadBank());
    });
  }
  widget.addEventListener("click", () => {
    const expanded = widget.getAttribute("aria-expanded") === "true";
    widget.setAttribute("aria-expanded", String(!expanded));
  });
  widget.addEventListener("keydown", (event) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    const expanded = widget.getAttribute("aria-expanded") === "true";
    widget.setAttribute("aria-expanded", String(!expanded));
  });
}
