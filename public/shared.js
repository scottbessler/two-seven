import { useEffect, useRef, useState } from "/public/vendor/htm-preact.js";

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
  const response = await fetch("/api/bank", { headers: { Accept: "application/json" }, cache: "no-store" });
  if (!response.ok) return null;
  const account = await response.json();
  announceBank(account);
  return account;
}

// A label clipped to an ellipsis is unreadable without its full text, so hand
// the browser a native tooltip — but only while the element is actually
// truncated, so untruncated controls stay tooltip-free.
export function useOverflowTitle(label) {
  const ref = useRef(null);
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const sync = () => {
      const truncated = element.scrollWidth > element.clientWidth + 1;
      if (truncated) element.title = label;
      else element.removeAttribute("title");
    };
    sync();
    if (typeof ResizeObserver !== "function") return;
    const observer = new ResizeObserver(sync);
    observer.observe(element);
    return () => observer.disconnect();
  }, [label]);
  return ref;
}

// A click over a laggy network looks like nothing happened, so people click
// again -- and a second bet is not a harmless repeat. One request at a time:
// the pressed control names itself while it is in flight, and the rest of the
// group stays inert until the answer lands.
export function usePending() {
  const [pending, setPending] = useState(null);
  const inFlight = useRef(null);
  const alive = useRef(true);
  useEffect(() => () => { alive.current = false; }, []);
  const run = async (key, action) => {
    if (inFlight.current != null) return;
    inFlight.current = key;
    setPending(key);
    try {
      await action();
    } finally {
      inFlight.current = null;
      if (alive.current) setPending(null);
    }
  };
  return [pending, run];
}
