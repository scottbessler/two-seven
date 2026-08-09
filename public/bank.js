const balance = document.getElementById("bank-balance");
const widget = document.querySelector(".bank-widget");
if (balance) {
  fetch("/api/bank", { headers: { Accept: "application/json" } })
    .then((response) => (response.ok ? response.json() : null))
    .then((account) => {
      if (!account) return;
      balance.textContent = `$${(account.balance / 100).toFixed(2)}`;
      if (widget) {
        const panel = document.createElement("span");
        panel.className = "bank-panel";
        panel.innerHTML = `<strong>Balance ${balance.textContent}</strong><br>${account.entries.slice(-5).map((entry) => `${entry.delta >= 0 ? "+" : ""}$${(entry.delta / 100).toFixed(2)} ${entry.memo}`).reduceRight((lines, line) => `${lines}${lines ? "<br>" : ""}${line}`, "")}`;
        widget.append(panel);
      }
    })
    .catch(() => {});
}
