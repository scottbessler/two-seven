export function cents(value) {
  return Math.round(Number(value) * 100);
}

export function money(value) {
  const sign = value < 0 ? "-" : "";
  const abs = Math.abs(value);
  return `${sign}$${Math.floor(abs / 100).toLocaleString()}.${String(abs % 100).padStart(2, "0")}`;
}

export function wholeDollarMoney(value) {
  const sign = value < 0 ? "-" : "";
  return `${sign}$${Math.round(Math.abs(value) / 100).toLocaleString()}`;
}

export async function responseError(response) {
  const text = await response.text();
  try {
    return JSON.parse(text).error || text;
  } catch {
    const document = new DOMParser().parseFromString(text, "text/html");
    return document.querySelector("p")?.textContent?.trim() || text || `Request failed (${response.status})`;
  }
}

export function announceBank(account) {
  window.dispatchEvent(new CustomEvent("bank:updated", { detail: account }));
}

export async function refreshBank() {
  const response = await fetch("/api/bank", { headers: { Accept: "application/json" } });
  if (!response.ok) return null;
  const account = await response.json();
  announceBank(account);
  return account;
}
